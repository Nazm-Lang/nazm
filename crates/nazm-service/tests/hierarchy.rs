//! Call hierarchy through the language service. N25.
//!
//! Every item is compared by where it is declared — its file and its name's bytes — and
//! every edge by the exact bytes of each call's callee name, never by a name alone, so that
//! two modules' `helper`, a record and a function both called `Box`, or a call taken for a
//! construction fails. Every expected set is written out exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::{HierarchyFunction, LanguageService, Locator, NoHierarchy};

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

/// The bytes of the name `name` at the start of the `n`th occurrence of `needle`.
fn name(text: &str, needle: &str, n: usize, name: &str) -> (u32, u32) {
    let (start, _) = span(text, needle, n);
    (start, start + u32::try_from(name.len()).unwrap())
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// An item as the tests compare it: its file, its name's bytes, and its name.
type Item = (String, (u32, u32), String);

fn item(f: &HierarchyFunction) -> Item {
    (
        file_name(&f.locator.file),
        (f.locator.name_start, f.locator.name_end),
        f.name.clone(),
    )
}

/// An edge as the tests compare it: the item at the other end, and every call's bytes,
/// with the file they are in.
type Edge = (Item, String, Vec<(u32, u32)>);

fn edge(f: &HierarchyFunction, calls: &[nazm_service::Location]) -> Edge {
    let files: BTreeSet<String> = calls.iter().map(|l| file_name(&l.path)).collect();
    assert_eq!(files.len(), 1, "one edge's calls are in one caller's file");
    (
        item(f),
        files.into_iter().next().unwrap(),
        calls.iter().map(|l| (l.start, l.end)).collect(),
    )
}

fn incoming(s: &LanguageService, l: &Locator) -> Vec<Edge> {
    s.incoming_calls(l)
        .unwrap()
        .iter()
        .map(|c| edge(&c.from, &c.calls))
        .collect()
}

fn outgoing(s: &LanguageService, l: &Locator) -> Vec<Edge> {
    s.outgoing_calls(l)
        .unwrap()
        .iter()
        .map(|c| edge(&c.to, &c.calls))
        .collect()
}

fn prepare(s: &LanguageService, path: &Path, version: i32, at: (u32, u32)) -> HierarchyFunction {
    s.prepare_call_hierarchy(path, version, at.0)
        .unwrap()
        .unwrap_or_else(|| panic!("nothing to prepare at {at:?}"))
}

fn it(file: &str, at: (u32, u32), n: &str) -> Item {
    (file.to_owned(), at, n.to_owned())
}

fn e(to: Item, file: &str, calls: &[(u32, u32)]) -> Edge {
    (to, file.to_owned(), calls.to_vec())
}

// ---------------------------------------------------------------------------------------
// Preparing an item.
// ---------------------------------------------------------------------------------------

const PREP: &str = "\
struct Box {
    value: Int,
}
enum State {
    Ready,
    Done(code: Int),
}
fn Box(x: Int) -> Int { x }
fn helper(n: Int) -> Int {
    let local = Box(n);
    let b = Box(value: local);
    let s = State.Done(code: b.value);
    let v = ints_new();
    helper(local)
}
";

#[test]
fn an_item_is_the_function_a_declaration_or_a_call_names_and_nothing_else() {
    let p = Project::new(&[("main.nz", PREP)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, PREP.to_owned());
        let fn_box = it("main.nz", name(PREP, "Box(x", 0, "Box"), "Box");
        let helper = it("main.nz", name(PREP, "helper(n", 0, "helper"), "helper");
        let at = |needle: &str, n: usize| {
            let (start, _) = span(PREP, needle, n);
            s.prepare_call_hierarchy(&main, 1, start)
                .unwrap()
                .map(|f| item(&f))
        };
        // A declaration, and a call of it, prepare the same function; a cursor just after a
        // name is on it, as everywhere else.
        assert_eq!(at("Box(x", 0), Some(fn_box.clone()));
        assert_eq!(at("Box(n)", 0), Some(fn_box.clone()));
        assert_eq!(at("helper(n", 0), Some(helper.clone()));
        assert_eq!(at("helper(local)", 0), Some(helper.clone()));
        let (_, end) = span(PREP, "helper(local", 0);
        let after = s.prepare_call_hierarchy(&main, 1, end - 6).unwrap();
        assert_eq!(after.map(|f| item(&f)), Some(helper));
        // The record of the same spelling, its construction, a field, an enum, a variant, a
        // payload label, a local, a parameter and an intrinsic are not functions with
        // source: none prepares anything.
        for (needle, n) in [
            ("Box {", 0),
            ("Box(value", 0),
            ("value: Int", 0),
            ("value)", 0),
            ("State {", 0),
            ("State.Done", 0),
            ("Done(code: b", 0),
            ("code: b", 0),
            ("local = ", 0),
            ("n: Int) -> Int {\n    let", 0),
            ("ints_new", 0),
            ("Int {", 0),
        ] {
            assert_eq!(at(needle, n), None, "{needle:?}");
        }
        // A superseded version prepares nothing; there is no previous answer.
        assert_eq!(
            s.prepare_call_hierarchy(&main, 2, 0),
            Err(NoHierarchy::NotCurrent)
        );
    });
}

#[test]
fn an_item_is_where_its_outline_symbol_is() {
    let p = Project::new(&[("main.nz", PREP)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, PREP.to_owned());
        let symbols = s.document_symbols(&main, 1).unwrap().symbols;
        let f = prepare(&s, &main, 1, span(PREP, "helper(n", 0));
        let sym = symbols
            .iter()
            .find(|x| x.name_start == f.selection.start)
            .unwrap();
        assert_eq!(
            (f.declaration.start, f.declaration.end),
            (sym.start, sym.end)
        );
        assert_eq!(
            (f.selection.start, f.selection.end),
            (sym.name_start, sym.name_end)
        );
        assert_eq!(f.detail, sym.detail);
        assert_eq!(f.detail.as_deref(), Some("fn helper(n: Int) -> Int"));
        assert_eq!(f.declaration.path, main);
        let (start, _) = span(PREP, "fn helper", 0);
        assert_eq!(f.declaration.start, start);
        assert!(PREP[..f.declaration.end as usize].ends_with("helper(local)\n}"));
    });
}

// ---------------------------------------------------------------------------------------
// Edges.
// ---------------------------------------------------------------------------------------

const EDGES: &str = "\
fn target() -> Int { 0 }
fn x() -> Int { 1 }
fn y() -> Int { 2 }
fn a() -> Int {
    target();
    target()
}
fn b() -> Int { target() }
fn caller() -> Int {
    x(); x();
    y()
}
fn outer(n: Int) -> Int { n }
fn inner(n: Int) -> Int { n }
enum Pick {
    Left,
    Right,
}
fn nested(c: Bool, p: Pick) -> Int {
    if c {
        while c {
            let k = outer(inner(1));
        }
    }
    match p {
        Pick.Left() => { outer(2) },
        Pick.Right() => inner(3),
    }
}
";

#[test]
fn calls_are_grouped_by_the_function_at_the_other_end_each_at_its_callee() {
    let p = Project::new(&[("main.nz", EDGES)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, EDGES.to_owned());
        let f = |needle: &str, n: &str| it("main.nz", name(EDGES, needle, 0, n), n);
        let target = prepare(&s, &main, 1, span(EDGES, "target()", 0));
        // Two callers: `a` twice, on two lines, and `b` once — two edges, not three.
        assert_eq!(
            incoming(&s, &target.locator),
            [
                e(
                    f("a()", "a"),
                    "main.nz",
                    &[
                        name(EDGES, "target()", 1, "target"),
                        name(EDGES, "target()", 2, "target")
                    ]
                ),
                e(
                    f("b()", "b"),
                    "main.nz",
                    &[name(EDGES, "target()", 3, "target")]
                ),
            ]
        );
        assert_eq!(outgoing(&s, &target.locator), []);
        // One caller of two callees: `x` twice on one line, `y` once.
        let caller = prepare(&s, &main, 1, span(EDGES, "caller", 0));
        assert_eq!(
            outgoing(&s, &caller.locator),
            [
                e(
                    f("x()", "x"),
                    "main.nz",
                    &[name(EDGES, "x()", 1, "x"), name(EDGES, "x()", 2, "x")]
                ),
                e(f("y()", "y"), "main.nz", &[name(EDGES, "y()", 1, "y")]),
            ]
        );
        // A call inside another call's arguments, inside `if`, `while` and `match`, is a
        // call of the function it is written in — never of the call around it.
        let nested = prepare(&s, &main, 1, span(EDGES, "nested", 0));
        assert_eq!(
            outgoing(&s, &nested.locator),
            [
                e(
                    f("outer(n", "outer"),
                    "main.nz",
                    &[
                        name(EDGES, "outer(inner", 0, "outer"),
                        name(EDGES, "outer(2", 0, "outer")
                    ]
                ),
                e(
                    f("inner(n", "inner"),
                    "main.nz",
                    &[
                        name(EDGES, "inner(1", 0, "inner"),
                        name(EDGES, "inner(3", 0, "inner")
                    ]
                ),
            ]
        );
        let outer = prepare(&s, &main, 1, span(EDGES, "outer(n", 0));
        assert_eq!(outgoing(&s, &outer.locator), []);
        let inner = prepare(&s, &main, 1, span(EDGES, "inner(n", 0));
        assert_eq!(
            incoming(&s, &inner.locator),
            [e(
                f("nested", "nested"),
                "main.nz",
                &[
                    name(EDGES, "inner(1", 0, "inner"),
                    name(EDGES, "inner(3", 0, "inner")
                ]
            )]
        );
    });
}

const RECURSIVE: &str = "\
fn f(n: Int) -> Int { if n > 0 { f(n - 1) } else { 0 } }
fn even(n: Int) -> Bool { if n == 0 { true } else { odd(n - 1) } }
fn odd(n: Int) -> Bool { if n == 0 { false } else { even(n - 1) } }
fn identity[T](x: T) -> T { x }
fn generic() -> Int {
    let s = identity[Str](\"x\");
    let b = identity(true);
    identity[Int](1)
}
";

#[test]
fn recursion_is_an_edge_and_instantiations_are_one_function() {
    let p = Project::new(&[("main.nz", RECURSIVE)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, RECURSIVE.to_owned());
        let f = |needle: &str, n: &str| it("main.nz", name(RECURSIVE, needle, 0, n), n);
        // A call of itself: in its incoming calls and its outgoing calls.
        let rec = prepare(&s, &main, 1, span(RECURSIVE, "f(n: Int)", 0));
        let self_edge = [e(
            f("f(n: Int)", "f"),
            "main.nz",
            &[name(RECURSIVE, "f(n - 1)", 0, "f")],
        )];
        assert_eq!(incoming(&s, &rec.locator), self_edge);
        assert_eq!(outgoing(&s, &rec.locator), self_edge);
        // Mutual recursion, one level each way.
        let even = prepare(&s, &main, 1, span(RECURSIVE, "even(n: Int)", 0));
        let odd_calls = [e(
            f("odd(n: Int)", "odd"),
            "main.nz",
            &[name(RECURSIVE, "odd(n - 1)", 0, "odd")],
        )];
        let even_calls = [e(
            f("even(n: Int)", "even"),
            "main.nz",
            &[name(RECURSIVE, "even(n - 1)", 0, "even")],
        )];
        assert_eq!(outgoing(&s, &even.locator), odd_calls);
        assert_eq!(
            incoming(&s, &even.locator),
            [e(
                f("odd(n: Int)", "odd"),
                "main.nz",
                &[name(RECURSIVE, "even(n - 1)", 0, "even")]
            )]
        );
        let odd = prepare(&s, &main, 1, span(RECURSIVE, "odd(n: Int)", 0));
        assert_eq!(outgoing(&s, &odd.locator), even_calls);
        // Three instantiations, written and inferred: one function, three calls, each at
        // the callee's name alone — never with its type arguments.
        let generic = prepare(&s, &main, 1, span(RECURSIVE, "generic", 0));
        let calls = [
            name(RECURSIVE, "identity[Str]", 0, "identity"),
            name(RECURSIVE, "identity(true", 0, "identity"),
            name(RECURSIVE, "identity[Int]", 0, "identity"),
        ];
        assert_eq!(
            outgoing(&s, &generic.locator),
            [e(f("identity[T]", "identity"), "main.nz", &calls)]
        );
        let identity = prepare(&s, &main, 1, span(RECURSIVE, "identity[Int]", 0));
        assert_eq!(item(&identity), f("identity[T]", "identity"));
        assert_eq!(
            incoming(&s, &identity.locator),
            [e(f("generic", "generic"), "main.nz", &calls)]
        );
    });
}

const NOT_CALLS: &str = "\
struct Box {
    value: Int,
}
enum State {
    Ready,
    Done(code: Int),
}
fn Box(x: Int) -> Int { x }
fn worker(n: Int) -> Int { n }
fn identity[T](x: T) -> T { x }
fn mixed() -> Int {
    let b = Box(value: 1);
    let s = State.Done(code: 2);
    let r = State.Ready();
    let v = ints_new();
    ints_push(v, 3);
    scope {
        spawn worker(4);
        spawn identity(5);
    }
    Box(b.value)
}
";

#[test]
fn constructions_and_intrinsics_are_not_edges_and_a_spawn_is_a_call() {
    let p = Project::new(&[("main.nz", NOT_CALLS)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, NOT_CALLS.to_owned());
        let f = |needle: &str, n: &str| it("main.nz", name(NOT_CALLS, needle, 0, n), n);
        let mixed = prepare(&s, &main, 1, span(NOT_CALLS, "mixed", 0));
        // The function `Box` is called once; the record `Box` and both variants are
        // constructed, which is not a call; the intrinsics have no source. `spawn worker`
        // is a checked call of `worker`. `spawn identity` is refused — a task may not run a
        // generic function — and the checker checked no call there, though the name
        // resolved: it is no edge.
        assert_eq!(
            outgoing(&s, &mixed.locator),
            [
                e(
                    f("Box(x", "Box"),
                    "main.nz",
                    &[name(NOT_CALLS, "Box(b.value", 0, "Box")]
                ),
                e(
                    f("worker(n", "worker"),
                    "main.nz",
                    &[name(NOT_CALLS, "worker(4", 0, "worker")]
                ),
            ]
        );
        let identity = prepare(&s, &main, 1, span(NOT_CALLS, "identity(5", 0));
        assert_eq!(item(&identity), f("identity[T]", "identity"));
        assert_eq!(incoming(&s, &identity.locator), []);
        // The reference index does count that spawn as a use: a reference is not a call.
        let refs = s
            .references(&main, 1, span(NOT_CALLS, "identity[T]", 0).0)
            .unwrap();
        assert_eq!(
            refs.uses
                .iter()
                .map(|l| (l.start, l.end))
                .collect::<Vec<_>>(),
            [name(NOT_CALLS, "identity(5", 0, "identity")]
        );
        let worker = prepare(&s, &main, 1, span(NOT_CALLS, "worker(n", 0));
        assert_eq!(
            incoming(&s, &worker.locator),
            [e(
                f("mixed", "mixed"),
                "main.nz",
                &[name(NOT_CALLS, "worker(4", 0, "worker")]
            )]
        );
    });
}

// ---------------------------------------------------------------------------------------
// Across files.
// ---------------------------------------------------------------------------------------

const ROOT: &str = "use \"a.nz\";\nfn entry() -> Int { helper(1) }\n";
const A: &str = "use \"b.nz\";\npub fn helper(n: Int) -> Int { if n > 0 { work(n) } else { own() } }\nfn own() -> Int { 0 }\n";
const B: &str =
    "use \"a.nz\";\nuse \"c.nz\";\npub fn work(n: Int) -> Int { helper(n - 1) + uses() }\n";
const C: &str = "fn helper() -> Int { 7 }\npub fn uses() -> Int { helper() }\n";

#[test]
fn edges_cross_files_and_same_named_functions_stay_apart() {
    // `root` calls `a.helper`; `a.helper` calls `b.work` and its private `own`; `b.work`
    // calls back into `a.helper` through an import cycle, and calls `c.uses`, which calls
    // `c`'s private `helper`. Two `helper`s, never one.
    let p = Project::new(&[("root.nz", ROOT), ("a.nz", A), ("b.nz", B), ("c.nz", C)]);
    let root = p.path("root.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&root, 1, ROOT.to_owned());
        assert_eq!(published[0].diagnostics, []);
        let a_helper = it("a.nz", name(A, "helper(n", 0, "helper"), "helper");
        let c_helper = it("c.nz", name(C, "helper()", 0, "helper"), "helper");
        let work = it("b.nz", name(B, "work(n", 0, "work"), "work");
        let uses = it("c.nz", name(C, "uses", 0, "uses"), "uses");
        let helper = prepare(&s, &root, 1, span(ROOT, "helper(1)", 0));
        assert_eq!(item(&helper), a_helper);
        assert_eq!(helper.declaration.path, p.path("a.nz"));
        assert_eq!(helper.locator.root, root.canonicalize().unwrap());
        assert_eq!(
            incoming(&s, &helper.locator),
            [
                e(
                    work.clone(),
                    "b.nz",
                    &[name(B, "helper(n - 1)", 0, "helper")]
                ),
                e(
                    it("root.nz", name(ROOT, "entry", 0, "entry"), "entry"),
                    "root.nz",
                    &[name(ROOT, "helper(1)", 0, "helper")]
                ),
            ]
        );
        let out = s.outgoing_calls(&helper.locator).unwrap();
        assert_eq!(
            out.iter()
                .map(|c| edge(&c.to, &c.calls))
                .collect::<Vec<_>>(),
            [
                e(
                    it("a.nz", name(A, "own() ->", 0, "own"), "own"),
                    "a.nz",
                    &[name(A, "own() }", 0, "own")]
                ),
                e(work.clone(), "a.nz", &[name(A, "work(n)", 0, "work")]),
            ]
        );
        // Followed one level at a time, from the items the answers hand back: `b.work`
        // calls `a.helper` back and `c.uses`, which calls `c`'s own `helper` — whose only
        // caller is `c.uses`.
        let work_item = &out[1].to;
        let out = s.outgoing_calls(&work_item.locator).unwrap();
        assert_eq!(
            out.iter()
                .map(|c| edge(&c.to, &c.calls))
                .collect::<Vec<_>>(),
            [
                e(
                    a_helper.clone(),
                    "b.nz",
                    &[name(B, "helper(n - 1)", 0, "helper")]
                ),
                e(uses.clone(), "b.nz", &[name(B, "uses()", 0, "uses")]),
            ]
        );
        let out = s.outgoing_calls(&out[1].to.locator).unwrap();
        assert_eq!(
            out.iter()
                .map(|c| edge(&c.to, &c.calls))
                .collect::<Vec<_>>(),
            [e(
                c_helper.clone(),
                "c.nz",
                &[name(C, "helper()", 1, "helper")]
            )]
        );
        assert_eq!(
            incoming(&s, &out[0].to.locator),
            [e(uses, "c.nz", &[name(C, "helper()", 1, "helper")])]
        );
        assert_eq!(out[0].to.declaration.path, p.path("c.nz"));
        // A locator is a declaration's exact bytes: one byte off names no declaration, and
        // nothing is looked up by the name it spells.
        let mut off = helper.locator.clone();
        off.name_start += 1;
        assert_eq!(s.incoming_calls(&off), Err(NoHierarchy::NotCurrent));
        assert_eq!(s.outgoing_calls(&off), Err(NoHierarchy::NotCurrent));
    });
}

const TWIN_ROOT: &str = "use \"a.nz\";\nuse \"c.nz\";\nfn entry() -> Int { helper() + run() }\n";
const TWIN_T: &str = "pub fn t() -> Int { 0 }\n";
const TWIN_A: &str = "use \"t.nz\";\npub fn helper() -> Int { t() }\n";
const TWIN_C: &str =
    "use \"t.nz\";\nfn helper() -> Int { t() }\npub fn run() -> Int { helper() }\n";

#[test]
fn two_callers_of_one_spelling_are_two_callers() {
    // `a.helper` and `c`'s private `helper` both call `t`: two edges, each from its own
    // declaration, never merged under the name they share.
    let p = Project::new(&[
        ("root.nz", TWIN_ROOT),
        ("t.nz", TWIN_T),
        ("a.nz", TWIN_A),
        ("c.nz", TWIN_C),
    ]);
    let root = p.path("root.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&root, 1, TWIN_ROOT.to_owned());
        assert_eq!(published[0].diagnostics, []);
        let run = prepare(&s, &root, 1, span(TWIN_ROOT, "run", 0));
        let c_helper = s.outgoing_calls(&run.locator).unwrap().remove(0).to;
        assert_eq!(
            item(&c_helper),
            it("c.nz", name(TWIN_C, "helper", 0, "helper"), "helper")
        );
        let t = s.outgoing_calls(&c_helper.locator).unwrap().remove(0).to;
        assert_eq!(
            incoming(&s, &t.locator),
            [
                e(
                    it("a.nz", name(TWIN_A, "helper", 0, "helper"), "helper"),
                    "a.nz",
                    &[name(TWIN_A, "t()", 0, "t")]
                ),
                e(
                    it("c.nz", name(TWIN_C, "helper", 0, "helper"), "helper"),
                    "c.nz",
                    &[name(TWIN_C, "t()", 0, "t")]
                ),
            ]
        );
    });
}

const COMMON: &str = "pub fn target() -> Int { 0 }\npub fn inside() -> Int { target() }\n";
const ROOT_A: &str = "use \"common.nz\";\nfn only_a() -> Int { target() }\n";
const ROOT_B: &str = "use \"common.nz\";\nfn only_b() -> Int { target() + target() }\n";

#[test]
fn an_item_answers_from_the_compilation_it_was_prepared_in_and_no_other() {
    let p = Project::new(&[("common.nz", COMMON), ("a.nz", ROOT_A), ("b.nz", ROOT_B)]);
    let (a, b, common) = (p.path("a.nz"), p.path("b.nz"), p.path("common.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&a, 1, ROOT_A.to_owned());
        s.open(&b, 1, ROOT_B.to_owned());
        let target = it("common.nz", name(COMMON, "target", 0, "target"), "target");
        let inside = e(
            it("common.nz", name(COMMON, "inside", 0, "inside"), "inside"),
            "common.nz",
            &[name(COMMON, "target()", 1, "target")],
        );
        let via_a = prepare(&s, &a, 1, span(ROOT_A, "target", 0));
        let via_b = prepare(&s, &b, 1, span(ROOT_B, "target", 0));
        // One declaration, two items: the same place, bound to two compilations.
        assert_eq!(item(&via_a), target);
        assert_eq!(item(&via_b), target);
        assert_eq!(via_a.declaration, via_b.declaration);
        assert_ne!(via_a.locator, via_b.locator);
        assert_eq!(
            incoming(&s, &via_a.locator),
            [
                e(
                    it("a.nz", name(ROOT_A, "only_a", 0, "only_a"), "only_a"),
                    "a.nz",
                    &[name(ROOT_A, "target", 0, "target")]
                ),
                inside.clone(),
            ]
        );
        assert_eq!(
            incoming(&s, &via_b.locator),
            [
                e(
                    it("b.nz", name(ROOT_B, "only_b", 0, "only_b"), "only_b"),
                    "b.nz",
                    &[
                        name(ROOT_B, "target", 0, "target"),
                        name(ROOT_B, "target", 1, "target")
                    ]
                ),
                inside.clone(),
            ]
        );
        // Opening the shared file makes a third compilation, which imports neither root.
        s.open(&common, 1, COMMON.to_owned());
        let own = prepare(&s, &common, 1, span(COMMON, "target", 0));
        assert_eq!(incoming(&s, &own.locator), [inside]);
        // Once `b` is closed its item has no answer — even with the generation brought up
        // to date by hand — and it is never answered from `a`, which still loads the file.
        let via_b = prepare(&s, &b, 1, span(ROOT_B, "target", 0));
        s.close(&b);
        let now = prepare(&s, &a, 1, span(ROOT_A, "target", 0))
            .locator
            .generation;
        let mut forged = via_b.locator.clone();
        forged.generation = now;
        for stale in [&via_b.locator, &forged] {
            assert_eq!(s.incoming_calls(stale), Err(NoHierarchy::NotCurrent));
            assert_eq!(s.outgoing_calls(stale), Err(NoHierarchy::NotCurrent));
        }
    });
}

// ---------------------------------------------------------------------------------------
// The current source, and only the current source.
// ---------------------------------------------------------------------------------------

const TYPED: &str = "\
fn g(n: Int) -> Int { n }
fn h(n: Int) -> Int { n }
fn f() -> Int {
    let a = g(\"not an Int\");
    h(1)
}
";

#[test]
fn a_call_with_a_type_error_is_an_edge_and_an_unchecked_call_is_none() {
    let p = Project::new(&[("main.nz", TYPED)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, TYPED.to_owned());
        assert!(
            !published[0].diagnostics.is_empty(),
            "the argument is a `Str`"
        );
        let f = |t: &str, needle: &str, n: &str| it("main.nz", name(t, needle, 0, n), n);
        let caller = prepare(&s, &main, 1, span(TYPED, "f()", 0));
        // The checker checked `g("…")` against `g`, and reported the argument: it is still
        // a call of `g`.
        let both = [
            e(
                f(TYPED, "g(n", "g"),
                "main.nz",
                &[name(TYPED, "g(\"", 0, "g")],
            ),
            e(
                f(TYPED, "h(n", "h"),
                "main.nz",
                &[name(TYPED, "h(1", 0, "h")],
            ),
        ];
        assert_eq!(outgoing(&s, &caller.locator), both);

        // `g` misspelt: that call resolved to nothing, so nothing was checked and there is
        // no edge — and the call of `h` beside it is still one.
        let typo = TYPED.replace("g(\"not", "gg(\"not");
        s.change(&main, 2, typo.clone());
        assert_eq!(
            s.incoming_calls(&caller.locator),
            Err(NoHierarchy::NotCurrent)
        );
        let caller = prepare(&s, &main, 2, span(&typo, "f()", 0));
        assert_eq!(
            outgoing(&s, &caller.locator),
            [e(
                f(&typo, "h(n", "h"),
                "main.nz",
                &[name(&typo, "h(1", 0, "h")]
            )]
        );
        let g = prepare(&s, &main, 2, span(&typo, "g(n", 0));
        assert_eq!(incoming(&s, &g.locator), []);

        // A body the parser cannot read keeps its declaration and loses every call in it:
        // nothing from the analysis before is brought back.
        let broken = TYPED.replace("h(1)", "h(1) +");
        s.change(&main, 3, broken.clone());
        let caller = prepare(&s, &main, 3, span(&broken, "f()", 0));
        assert_eq!(outgoing(&s, &caller.locator), []);
        let h = prepare(&s, &main, 3, span(&broken, "h(n", 0));
        assert_eq!(incoming(&s, &h.locator), []);

        // Repaired: the edges are the checker's again.
        s.change(&main, 4, TYPED.to_owned());
        let caller = prepare(&s, &main, 4, span(TYPED, "f()", 0));
        assert_eq!(outgoing(&s, &caller.locator), both);
    });
}

const DUPLICATE: &str = "\
fn a() -> Int { 1 }
fn b() -> Int { 2 }
fn twice() -> Int { a() }
fn twice() -> Int { b() }
";

#[test]
fn a_refused_second_declaration_is_no_item_and_its_calls_are_the_checkers() {
    // The checker refuses the second `twice` and checks its body as the one `twice` the
    // module has — so its call of `b` is recorded as a call from that function. The
    // hierarchy follows the checker: one `twice`, prepared from either name, with both
    // calls, and no second item.
    let p = Project::new(&[("main.nz", DUPLICATE)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, DUPLICATE.to_owned());
        let twice = it("main.nz", name(DUPLICATE, "twice", 0, "twice"), "twice");
        let first = prepare(&s, &main, 1, span(DUPLICATE, "twice", 0));
        assert_eq!(item(&first), twice);
        let second = s
            .prepare_call_hierarchy(&main, 1, span(DUPLICATE, "twice", 1).0)
            .unwrap();
        assert_eq!(
            second.map(|f| item(&f)),
            None,
            "the refused name is no function"
        );
        let b = prepare(&s, &main, 1, span(DUPLICATE, "b()", 0));
        assert_eq!(
            incoming(&s, &b.locator),
            [e(twice, "main.nz", &[name(DUPLICATE, "b()", 1, "b")])]
        );
    });
}

const CALLER: &str = "use \"lib.nz\";\nfn f() -> Int { g() }\n";
const CALLER_EDITED: &str = "use \"lib.nz\";\nfn f() -> Int { h() }\n";
const LIB: &str = "pub fn g() -> Int { 1 }\npub fn h() -> Int { 2 }\n";
const LIB_EDITED: &str = "pub fn g() -> Int { h() }\npub fn h() -> Int { 2 }\n";

#[test]
fn edges_follow_the_buffers_and_close_restores_the_disk() {
    let p = Project::new(&[("main.nz", CALLER), ("lib.nz", LIB)]);
    let (main, lib) = (p.path("main.nz"), p.path("lib.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, CALLER.to_owned());
        let g = it("lib.nz", name(LIB, "g()", 0, "g"), "g");
        let h = it("lib.nz", name(LIB, "h()", 0, "h"), "h");
        let f = prepare(&s, &main, 1, span(CALLER, "f()", 0));
        assert_eq!(
            outgoing(&s, &f.locator),
            [e(g.clone(), "main.nz", &[name(CALLER, "g()", 0, "g")])]
        );

        // The caller, unsaved: `g` gone, `h` in its place. The disk still says `g`.
        s.change(&main, 2, CALLER_EDITED.to_owned());
        let f = prepare(&s, &main, 2, span(CALLER_EDITED, "f()", 0));
        assert_eq!(
            outgoing(&s, &f.locator),
            [e(
                h.clone(),
                "main.nz",
                &[name(CALLER_EDITED, "h()", 0, "h")]
            )]
        );

        // The dependency, unsaved: `g` now calls `h`, seen from `main`'s compilation.
        s.change(&main, 3, CALLER.to_owned());
        s.open(&lib, 1, LIB_EDITED.to_owned());
        let h_edited = it("lib.nz", name(LIB_EDITED, "h() -> Int { 2", 0, "h"), "h");
        let hh = prepare(&s, &main, 3, span(CALLER, "g()", 0));
        assert_eq!(item(&hh), g);
        let h_item = s.outgoing_calls(&hh.locator).unwrap();
        assert_eq!(
            h_item
                .iter()
                .map(|c| edge(&c.to, &c.calls))
                .collect::<Vec<_>>(),
            [e(h_edited, "lib.nz", &[name(LIB_EDITED, "h()", 0, "h")])]
        );
        assert_eq!(
            incoming(&s, &h_item[0].to.locator),
            [e(g.clone(), "lib.nz", &[name(LIB_EDITED, "h()", 0, "h")])]
        );

        // Closed: the disk is the dependency again, and `g` calls nothing.
        s.close(&lib);
        let g_again = prepare(&s, &main, 3, span(CALLER, "g()", 0));
        assert_eq!(outgoing(&s, &g_again.locator), []);
    });
}

#[test]
fn a_prepared_item_has_no_answer_once_anything_changes() {
    const TEXT: &str = "fn g() -> Int { 1 }\nfn f() -> Int { g() }\n";
    let p = Project::new(&[("main.nz", TEXT), ("other.nz", "fn o() -> Int { 0 }\n")]);
    let (main, other) = (p.path("main.nz"), p.path("other.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, TEXT.to_owned());
        let f = prepare(&s, &main, 1, span(TEXT, "f()", 0));
        let edges = outgoing(&s, &f.locator);
        assert_eq!(edges.len(), 1);
        // A version the document is not at: no answer, even at the current generation.
        let mut wrong = f.locator.clone();
        wrong.version = 2;
        assert_eq!(s.outgoing_calls(&wrong), Err(NoHierarchy::NotCurrent));
        // An edit that does not touch `f` — here, one to another document entirely — still
        // ends the item: it belongs to the generation it was prepared at.
        s.open(&other, 1, "fn o() -> Int { 1 }\n".to_owned());
        assert_eq!(s.outgoing_calls(&f.locator), Err(NoHierarchy::NotCurrent));
        assert_eq!(s.incoming_calls(&f.locator), Err(NoHierarchy::NotCurrent));
        // So does a change to the document itself, identical text and all.
        let f = prepare(&s, &main, 1, span(TEXT, "f()", 0));
        s.change(&main, 2, TEXT.to_owned());
        assert_eq!(s.outgoing_calls(&f.locator), Err(NoHierarchy::NotCurrent));
        // Prepared again, it answers.
        let f = prepare(&s, &main, 2, span(TEXT, "f()", 0));
        assert_eq!(outgoing(&s, &f.locator), edges);
    });
}

#[test]
fn a_thousand_edits_leave_only_the_current_graph() {
    // Six shapes in turn: a call added, removed, moved to another callee, made recursive,
    // made through a changed import, broken and repaired. At every version a fresh item
    // answers with that version's edges, and the one prepared before the edit has none.
    const LIB: &str = "pub fn far() -> Int { 9 }\n";
    const SHAPES: [&str; 6] = [
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { g() }\n",
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { g() + g() }\n",
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { h() }\n",
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { f() }\n",
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { far() }\n",
        "use \"lib.nz\";\nfn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { g( }\n",
    ];
    let expected: [&[&str]; 6] = [&["g"], &["g"], &["h"], &["f"], &["far"], &[]];
    let p = Project::new(&[("main.nz", SHAPES[0]), ("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 0, SHAPES[0].to_owned());
        let baseline = (s.retained(), s.indexed(), s.calls());
        let mut previous: Option<Locator> = None;
        for v in 1..=1000 {
            let k = usize::try_from(v).unwrap() % SHAPES.len();
            s.change(&main, v, SHAPES[k].to_owned());
            if let Some(old) = previous.take() {
                assert_eq!(s.outgoing_calls(&old), Err(NoHierarchy::NotCurrent));
                assert_eq!(s.incoming_calls(&old), Err(NoHierarchy::NotCurrent));
            }
            assert_eq!(
                s.prepare_call_hierarchy(&main, v - 1, 0),
                Err(NoHierarchy::NotCurrent)
            );
            let f = prepare(&s, &main, v, span(SHAPES[k], "f()", 0));
            let callees: Vec<String> = s
                .outgoing_calls(&f.locator)
                .unwrap()
                .into_iter()
                .map(|c| c.to.name)
                .collect();
            assert_eq!(callees, expected[k], "version {v}");
            previous = Some(f.locator);
            if k == 0 {
                assert_eq!(
                    (s.retained(), s.indexed(), s.calls()),
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
fn every_checked_call_of_the_repository_is_one_edge_occurrence_that_every_layer_agrees_on() {
    // Over real source, for every document whose compilation checks cleanly: each checked
    // call to a function with source is exactly one occurrence in exactly one edge — from
    // the function whose body holds it, to the function the checker resolved it to, at
    // the callee's name — seen the same from both ends. And at every item, the outline
    // symbol (N22) has the same ranges, the reference index (N17) the same entity, and
    // the semantic identifiers (N23) say a function declared or referenced there.
    use nazm_core::sema::{Callee, ReferenceTarget};
    let root = workspace();
    let mut files = walk(&root.join("compiler"));
    files.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let (mut documents, mut functions, mut calls, mut edges) = (0, 0, 0, 0);
        let (mut builtins, mut recursive, mut generic, mut spawned) = (0, 0, 0, 0);
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
            let index = nazm_core::sema::ReferenceIndex::of(res);
            let prelude = loaded.graph.prelude();
            let text = std::fs::read_to_string(path).unwrap();
            let mut s = LanguageService::new();
            s.open(path, 1, text.clone());
            let identifiers = s.semantic_identifiers(path, 1).unwrap();
            let symbols = s.document_symbols(path, 1).unwrap().symbols;

            // Independently of the service: every checked call, by its callee span, to the
            // function whose body it is in — found by containment in the declarations the
            // parser read, not by the checker's per-body record.
            let body_of = |at: nazm_span::Span| {
                res.functions()
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| {
                        a.program.functions.get(d.decl as usize).is_some_and(|f| {
                            f.name_span == d.name_span
                                && f.span.file == at.file
                                && f.span.start <= at.start
                                && at.end <= f.span.end
                        })
                    })
                    .map(|(i, _)| nazm_core::sema::FnId::from_index(i))
                    .collect::<Vec<_>>()
            };
            let mut expected: BTreeSet<(u32, u32, u32, u32, u32)> = BTreeSet::new();
            for (at, call) in res.checked_calls() {
                calls += 1;
                let Callee::Function(to) = call.callee else {
                    builtins += 1;
                    continue;
                };
                let def = res.function(to).unwrap();
                // The prelude's and the standard library's functions (N45) are the
                // toolchain's: no file to open, so no item in a hierarchy.
                let module = loaded.graph.module_of(def.name_span.file);
                if module == prelude
                    || module
                        .and_then(|m| loaded.graph.key(m))
                        .is_some_and(nazm_core::sema::ModuleKey::is_std)
                {
                    builtins += 1;
                    continue;
                }
                let owners = body_of(at);
                assert_eq!(owners.len(), 1, "{}: {at:?} is in one body", path.display());
                let from = owners[0];
                if from == to {
                    recursive += 1;
                }
                if call.instance.is_some() {
                    generic += 1;
                }
                if call.spawned {
                    spawned += 1;
                }
                // N17: the reference index has the call's callee as the same function.
                assert_eq!(index.target(at), Some(ReferenceTarget::Function(to)));
                expected.insert((
                    u32::try_from(from.index()).unwrap(),
                    u32::try_from(to.index()).unwrap(),
                    u32::try_from(at.file.index()).unwrap(),
                    at.start,
                    at.end,
                ));
            }

            // The service's edges, from every function of the root file, both ways.
            let id_of = |f: &HierarchyFunction| -> u32 {
                let file = loaded
                    .map
                    .ids()
                    .find(|&id| {
                        Path::new(loaded.map.name(id))
                            .canonicalize()
                            .is_ok_and(|p| p == f.locator.file)
                    })
                    .unwrap_or_else(|| panic!("no loaded file is {}", f.locator.file.display()));
                let at = nazm_span::Span::new(file, f.locator.name_start, f.locator.name_end);
                match index.target(at) {
                    Some(ReferenceTarget::Function(id)) => u32::try_from(id.index()).unwrap(),
                    other => panic!("{other:?} at an item"),
                }
            };
            let file_of = |l: &nazm_service::Location| -> u32 {
                let id = loaded
                    .map
                    .ids()
                    .find(|&id| {
                        Path::new(loaded.map.name(id)).canonicalize().ok()
                            == l.path.canonicalize().ok()
                    })
                    .unwrap();
                u32::try_from(id.index()).unwrap()
            };
            let mut seen_out: BTreeSet<(u32, u32, u32, u32, u32)> = BTreeSet::new();
            let mut seen_in: BTreeSet<(u32, u32, u32, u32, u32)> = BTreeSet::new();
            let mut items: Vec<HierarchyFunction> = Vec::new();
            for symbol in &symbols {
                if symbol.target.kind() != "function" {
                    continue;
                }
                functions += 1;
                let f = prepare(&s, path, 1, (symbol.name_start, symbol.name_end));
                // N22: the same declaration, the same ranges.
                assert_eq!(
                    (
                        f.declaration.start,
                        f.declaration.end,
                        f.selection.start,
                        f.selection.end
                    ),
                    (symbol.start, symbol.end, symbol.name_start, symbol.name_end)
                );
                let me = id_of(&f);
                for c in s.outgoing_calls(&f.locator).unwrap() {
                    let to = id_of(&c.to);
                    items.push(c.to.clone());
                    for l in &c.calls {
                        assert!(
                            seen_out.insert((me, to, file_of(l), l.start, l.end)),
                            "one occurrence once"
                        );
                        edges += 1;
                    }
                }
                for c in s.incoming_calls(&f.locator).unwrap() {
                    let from = id_of(&c.from);
                    items.push(c.from.clone());
                    for l in &c.calls {
                        assert!(seen_in.insert((from, me, file_of(l), l.start, l.end)));
                    }
                }
                items.push(f);
            }
            // Every edge out of a root-file function, and every edge into one, is exactly the
            // checked calls with that end.
            let root_fns: BTreeSet<u32> = symbols
                .iter()
                .filter_map(|x| match x.target {
                    ReferenceTarget::Function(id) => Some(u32::try_from(id.index()).unwrap()),
                    _ => None,
                })
                .collect();
            let out_expected: BTreeSet<_> = expected
                .iter()
                .filter(|(from, ..)| root_fns.contains(from))
                .copied()
                .collect();
            let in_expected: BTreeSet<_> = expected
                .iter()
                .filter(|(_, to, ..)| root_fns.contains(to))
                .copied()
                .collect();
            assert_eq!(seen_out, out_expected, "{}", path.display());
            assert_eq!(seen_in, in_expected, "{}", path.display());

            // N23, in the root file: every item's name is a function declared there, and
            // every call a function referenced there.
            let class_at = |start: u32, end: u32| {
                identifiers
                    .identifiers
                    .iter()
                    .find(|i| i.start == start && i.end == end)
                    .map(|i| (i.class, i.role))
            };
            for f in items
                .iter()
                .filter(|f| f.locator.file == path.canonicalize().unwrap())
            {
                assert_eq!(
                    class_at(f.selection.start, f.selection.end),
                    Some((
                        nazm_service::SemanticClass::Function,
                        nazm_service::Role::Declaration
                    ))
                );
            }
            for (_, _, file, start, end) in seen_out.iter().chain(&seen_in) {
                if *file == 0 {
                    assert_eq!(
                        class_at(*start, *end),
                        Some((
                            nazm_service::SemanticClass::Function,
                            nazm_service::Role::Reference
                        ))
                    );
                }
            }
        }
        eprintln!(
            "{documents} documents: {functions} root-file functions, {calls} checked calls, {edges} outgoing edge occurrences from root-file functions, {builtins} to built-ins or the prelude, {recursive} recursive, {generic} generic, {spawned} spawned"
        );
        assert!(documents > 0 && edges > 0 && recursive > 0 && generic > 0 && builtins > 0);
    });
}
