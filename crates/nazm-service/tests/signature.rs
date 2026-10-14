//! The call at a position, and signature help from it, through the language service. N20.
//!
//! Every call is identified by where its callee is declared — through the reference index,
//! not by the name in the signature — so that a call recovered by spelling, or the record
//! of the same name taken for the function, fails. Every active parameter is written out at
//! every boundary that could move it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use nazm_core::sema::{Callee, Intrinsic, Type};
use nazm_service::{CallInfo, LanguageService, NoCall, SignatureHelp, Target};

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

fn help(s: &LanguageService, path: &Path, version: i32, offset: u32) -> SignatureHelp {
    s.signature_help(path, version, offset)
        .unwrap_or_else(|e| panic!("no signature help at {offset}: {e:?}"))
}

/// Where the call's callee is declared — file and byte — by the reference index, which is
/// the resolution read backwards: `None` for a built-in.
fn declared(
    s: &LanguageService,
    path: &Path,
    version: i32,
    call: &CallInfo,
) -> Option<(String, u32)> {
    let d = s.definition(path, version, call.callee_start)?;
    let Target::File(l) = d.target else {
        return None;
    };
    Some((
        l.path.file_name().unwrap().to_string_lossy().into_owned(),
        l.start,
    ))
}

/// The label and the active parameter, which is what a client shows.
type Shown<'a> = (&'a str, Option<u32>);

fn shown(h: &SignatureHelp) -> Shown<'_> {
    (h.rendered.label.as_str(), h.active_parameter)
}

// ---------------------------------------------------------------------------------------
// Which call: the one the checker checked.
// ---------------------------------------------------------------------------------------

const LIB: &str = "pub fn helper(n: Int) -> Int { n }\n";
const OTHER: &str = "fn helper(s: Str, t: Str) -> Str { s }\npub fn other_pub() -> Int { 1 }\n";
const MAIN: &str = "use \"lib.nz\";\n\
use \"other.nz\";\n\
struct Box { v: Int, }\n\
fn Box(x: Int) -> Int { x }\n\
fn identity[T](x: T) -> T { x }\n\
fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
fn main() -> Int {\n    \
    let helper = 5;\n    \
    let b = Box(v: helper);\n    \
    let n = Box(helper(identity(2)));\n    \
    let m = identity[Int](str_len(\"é, (x\"));\n    \
    if same(n, m) { b.v + other_pub() } else { helper }\n\
}\n";

#[test]
fn signature_help_follows_the_resolved_call_not_the_spelling() {
    let p = Project::new(&[("lib.nz", LIB), ("other.nz", OTHER), ("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let fn_box = at(MAIN, "Box(x", 0, 0);

        // `Box(…)` with an argument is the function, not the record of the same name.
        let h = help(&s, &main, 1, at(MAIN, "Box(helper", 0, 4));
        assert_eq!(shown(&h), ("fn Box(x: Int) -> Int", Some(0)));
        assert_eq!(
            declared(&s, &main, 1, &h.call),
            Some(("main.nz".into(), fn_box))
        );
        // `helper(…)` is the imported function — not the local `helper`, and not the
        // private `helper` of another imported module.
        let h = help(&s, &main, 1, at(MAIN, "helper(identity", 0, 7));
        assert_eq!(shown(&h), ("fn helper(n: Int) -> Int", Some(0)));
        assert_eq!(declared(&s, &main, 1, &h.call), Some(("lib.nz".into(), 7)));
        // The generic call, as it was inferred.
        let h = help(&s, &main, 1, at(MAIN, "identity(2)", 0, 9));
        assert_eq!(shown(&h), ("fn identity[Int](x: Int) -> Int", Some(0)));
        let instance = h.call.instance.as_ref().unwrap();
        assert!(!instance.written);
        assert_eq!(instance.args, [Some(Type::Int)]);
        assert_eq!(instance.params, [Some(Type::Int)]);
        assert_eq!(instance.ret, Some(Type::Int));
        // ...and as it was written.
        let h = help(&s, &main, 1, at(MAIN, "identity[Int](", 0, 14));
        assert_eq!(shown(&h), ("fn identity[Int](x: Int) -> Int", Some(0)));
        assert!(h.call.instance.as_ref().unwrap().written);
        // A built-in, from the compiler's own inventory: typed, not named.
        let h = help(&s, &main, 1, at(MAIN, "é, (x", 0, 3));
        assert_eq!(shown(&h), ("fn str_len(Str) -> Int", Some(0)));
        assert_eq!(h.call.callee, Callee::Builtin(Intrinsic::StrLen));
        assert_eq!(declared(&s, &main, 1, &h.call), None);
        // A requirement is part of the declared signature; the call shows what it settled.
        let h = help(&s, &main, 1, at(MAIN, "same(n, m)", 0, 8));
        assert_eq!(shown(&h), ("fn same[Int](a: Int, b: Int) -> Bool", Some(1)));
        assert!(h.call.signature.type_params[0].requires_equality);
        assert_eq!(
            h.call.signature.render(&analysis_of(&main), None).label,
            "fn same[T: Equality](a: T, b: T) -> Bool"
        );
        // A function with no parameters has a signature and no active parameter.
        let h = help(&s, &main, 1, at(MAIN, "other_pub()", 0, 10));
        assert_eq!(shown(&h), ("fn other_pub() -> Int", None));
        assert_eq!(h.call.active_argument, 0);
        // A record construction is not a call.
        assert_eq!(
            s.signature_help(&main, 1, at(MAIN, "Box(v: ", 0, 7))
                .unwrap_err(),
            NoCall::NotInACall
        );
    });
}

/// A fresh analysis of the program at `main`, for rendering a structured signature from
/// the service — which only reads resolution data, so any analysis of the same program
/// renders it the same way.
fn analysis_of(main: &Path) -> nazm_core::Resolution {
    let decided = nazm_service::root::decide(main, None).unwrap();
    let loaded = nazm_service::load::load(main, decided.path()).unwrap();
    nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent).resolution
}

#[test]
fn hover_and_signature_help_render_one_signature() {
    let p = Project::new(&[("lib.nz", LIB), ("other.nz", OTHER), ("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        for (needle, into) in [("Box(helper", 4), ("helper(identity", 7), ("same(n", 5)] {
            let call = s.call_at(&main, 1, at(MAIN, needle, 0, into)).unwrap();
            let hover = s.hover(&main, 1, call.callee_start).unwrap();
            let res = analysis_of(&main);
            // The signature is the first line; an undeclared function's inferred effects,
            // which are not a declaration, follow it (N36).
            assert_eq!(
                hover.text.lines().next(),
                Some(call.signature.render(&res, None).label.as_str()),
                "{needle}"
            );
        }
        let h = s.hover(&main, 1, at(MAIN, "same(n", 0, 0)).unwrap();
        assert_eq!(
            h.text,
            "fn same[T: Equality](a: T, b: T) -> Bool\ninferred effects: {}"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Which argument: the argument list's own separators.
// ---------------------------------------------------------------------------------------

const NESTED: &str = "fn three(a: Int, b: Int, c: Int) -> Int { a + b + c }\n\
fn pair[T, U](t: T, u: U) -> Int { 0 }\n\
struct P { x: Int, y: Int, }\n\
fn main() -> Int {\n    \
    three(\n        \
        str_len(\"a,b)\"), // one, two)\n        \
        (P(x: 1, y: 2).x),\n        \
        pair[P, Vec[Int]](P(x: 3, y: 4), vec_new[Int]())\n    \
    ) + three(1, 2, 3)\n\
}\n";

#[test]
fn the_active_parameter_is_the_argument_lists_own_structure() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, NESTED.into());
        let three = "fn three(a: Int, b: Int, c: Int) -> Int";
        let pair = "fn pair[P, Vec[Int]](t: P, u: Vec[Int]) -> Int";
        let str_len = "fn str_len(Str) -> Int";
        let vec_new = "fn vec_new[Int]() -> Vec[Int]";
        let open = at(NESTED, "three(\n", 0, 0);
        let close = at(NESTED, ") + three", 0, 0);
        let cases: [(&str, u32, Option<Shown<'_>>); 22] = [
            ("on the callee", open + 3, None),
            ("before the (", open + 5, None),
            ("after the (", open + 6, Some((three, Some(0)))),
            (
                "before the first argument",
                at(NESTED, "str_len", 0, 0),
                Some((three, Some(0))),
            ),
            (
                "in a nested call",
                at(NESTED, "str_len(", 0, 8),
                Some((str_len, Some(0))),
            ),
            (
                "a string's comma",
                at(NESTED, "a,b", 0, 2),
                Some((str_len, Some(0))),
            ),
            (
                "a string's paren",
                at(NESTED, "b)\"", 0, 2),
                Some((str_len, Some(0))),
            ),
            (
                "after the nested call",
                at(NESTED, "), //", 0, 1),
                Some((three, Some(0))),
            ),
            (
                "immediately after a comma",
                at(NESTED, "), //", 0, 2),
                Some((three, Some(1))),
            ),
            (
                "a comment's comma",
                at(NESTED, "one, two", 0, 5),
                Some((three, Some(1))),
            ),
            (
                "a comment's paren",
                at(NESTED, "two)", 0, 4),
                Some((three, Some(1))),
            ),
            (
                "a parenthesised argument",
                at(NESTED, "(P(x", 0, 1),
                Some((three, Some(1))),
            ),
            (
                "a construction's comma",
                at(NESTED, "y: 2", 0, 0),
                Some((three, Some(1))),
            ),
            (
                "the second separator",
                at(NESTED, ".x),", 0, 4),
                Some((three, Some(2))),
            ),
            (
                "type arguments",
                at(NESTED, "P, Vec", 0, 2),
                Some((three, Some(2))),
            ),
            (
                "nested type arguments",
                at(NESTED, "Int]]", 0, 3),
                Some((three, Some(2))),
            ),
            (
                "a generic call",
                at(NESTED, "P(x: 3", 0, 0),
                Some((pair, Some(0))),
            ),
            (
                "its second argument",
                at(NESTED, "vec_new", 0, 0),
                Some((pair, Some(1))),
            ),
            (
                "no parameters",
                at(NESTED, "[Int]()", 0, 6),
                Some((vec_new, None)),
            ),
            ("before the )", close, Some((three, Some(2)))),
            ("after the )", close + 1, None),
            ("the next call", at(NESTED, "2, 3", 0, 1), None),
        ];
        for (what, offset, expected) in cases {
            let got = s.signature_help(&m, 1, offset);
            match expected {
                None if what == "the next call" => {
                    // Not a miss: another call, whose own second argument this is.
                    let h = got.unwrap();
                    assert_eq!(shown(&h), (three, Some(1)), "{what}");
                    assert_ne!(h.call.callee_start, open, "{what}");
                }
                None => assert_eq!(got.unwrap_err(), NoCall::NotInACall, "{what}"),
                Some(e) => assert_eq!(shown(&got.unwrap()), e, "{what}"),
            }
        }
    });
}

// ---------------------------------------------------------------------------------------
// A call while it is being written.
// ---------------------------------------------------------------------------------------

#[test]
fn a_call_the_checker_rejected_still_has_the_callee_it_resolved() {
    let t = "fn two(a: Int, b: Str) -> Int { a }\n\
fn ping() -> Int { 0 }\n\
fn worker(c: Chan) -> Int { 0 }\n\
fn main(tasks: SpawnCap) -> Int {\n    \
    let c = chan_new(1);\n    \
    scope { spawn worker(c); }\n    \
    two() + two(1, ) + two(1, \"s\", 3) + two(\"x\", 1) + ping() + nope(1)\n\
}\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, t.into());
        let two = "fn two(a: Int, b: Str) -> Int";
        // Too few: nothing written yet, and after a trailing comma.
        assert_eq!(
            shown(&help(&s, &m, 1, at(t, "two() +", 0, 4))),
            (two, Some(0))
        );
        assert_eq!(
            shown(&help(&s, &m, 1, at(t, "two(1, )", 0, 7))),
            (two, Some(1))
        );
        // Too many: the argument past the last parameter is at no parameter.
        let h = help(&s, &m, 1, at(t, ", 3)", 0, 2));
        assert_eq!(shown(&h), (two, None));
        assert_eq!(h.call.active_argument, 2);
        assert_eq!(h.call.arguments.len(), 3);
        // The wrong type: the callee is still the one the checker resolved.
        assert_eq!(
            shown(&help(&s, &m, 1, at(t, "\"x\", 1", 0, 1))),
            (two, Some(0))
        );
        // No parameters.
        let h = help(&s, &m, 1, at(t, "ping() +", 0, 5));
        assert_eq!(shown(&h), ("fn ping() -> Int", None));
        // A callee that did not resolve has no signature, whatever it is spelled.
        assert_eq!(
            s.signature_help(&m, 1, at(t, "nope(1", 0, 5)).unwrap_err(),
            NoCall::NotChecked
        );
        // A `spawn` is a checked call too.
        let h = help(&s, &m, 1, at(t, "worker(c)", 0, 7));
        assert_eq!(shown(&h), ("fn worker(c: Chan) -> Int", Some(0)));
        assert!(h.call.spawned);
    });
}

// ---------------------------------------------------------------------------------------
// The current program, and no other.
// ---------------------------------------------------------------------------------------

#[test]
fn a_stale_version_has_no_call_information() {
    let v1 = "fn foo(a: Int, b: Int) -> Int { a }\nfn main() -> Int { foo(1, 2) }\n";
    let v2 = "fn foo(a: Int, b: Int, c: Int) -> Int { a }\nfn main() -> Int { foo(1, 2, 3) }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, v1.into());
        let second = at(v1, "2)", 0, 0);
        assert_eq!(
            shown(&help(&s, &m, 1, second)),
            ("fn foo(a: Int, b: Int) -> Int", Some(1))
        );
        s.change(&m, 2, v2.into());
        assert_eq!(
            s.signature_help(&m, 1, second).unwrap_err(),
            NoCall::NotCurrent
        );
        assert_eq!(s.call_at(&m, 1, second).unwrap_err(), NoCall::NotCurrent);
        assert_eq!(
            shown(&help(&s, &m, 2, at(v2, "3)", 0, 0))),
            ("fn foo(a: Int, b: Int, c: Int) -> Int", Some(2))
        );
    });
}

#[test]
fn broken_current_source_has_no_call_and_never_the_last_clean_one() {
    let clean = "fn foo(a: Int) -> Int { a }\nfn main() -> Int { foo(1) }\n";
    let broken = "fn foo(a: Int) -> Int { a }\nfn main() -> Int { let y = ; foo(1) }\n";
    let elsewhere = "fn foo(a: Int) -> Int { a }\nfn main() -> Int { foo(1) }\nfn x( {\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, clean.into());
        assert!(s.signature_help(&m, 1, at(clean, "1)", 0, 0)).is_ok());
        s.change(&m, 2, broken.into());
        assert_eq!(
            s.signature_help(&m, 2, at(broken, "1)", 0, 0)).unwrap_err(),
            NoCall::RecoveredSyntax
        );
        // A recovery anywhere in the compilation: a lost item could change what resolves.
        s.change(&m, 3, elsewhere.into());
        assert_eq!(
            s.signature_help(&m, 3, at(elsewhere, "1)", 0, 0))
                .unwrap_err(),
            NoCall::RecoveredSyntax
        );
        // An unclosed argument list is a syntax error too.
        let open = "fn foo(a: Int) -> Int { a }\nfn main() -> Int { foo(1 }\n";
        s.change(&m, 4, open.into());
        assert!(s.signature_help(&m, 4, at(open, "1 }", 0, 1)).is_err());
        s.change(&m, 5, clean.into());
        assert_eq!(
            shown(&help(&s, &m, 5, at(clean, "1)", 0, 0))),
            ("fn foo(a: Int) -> Int", Some(0))
        );
    });
}

#[test]
fn an_unsaved_dependency_signature_is_what_the_importer_shows() {
    let lib = "pub fn f(x: Int) -> Int { x }\n";
    let main = "use \"lib.nz\";\nfn main() -> Int { f(1) }\n";
    let p = Project::new(&[("lib.nz", lib), ("main.nz", main)]);
    let (pl, pm) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&pm, 1, main.into());
        let inside = at(main, "1)", 0, 0);
        assert_eq!(shown(&help(&s, &pm, 1, inside)).0, "fn f(x: Int) -> Int");
        s.open(&pl, 1, "pub fn f(x: Int, y: Int) -> Int { x }\n".into());
        assert_eq!(
            shown(&help(&s, &pm, 1, inside)),
            ("fn f(x: Int, y: Int) -> Int", Some(0))
        );
        s.close(&pl);
        assert_eq!(shown(&help(&s, &pm, 1, inside)).0, "fn f(x: Int) -> Int");
    });
}

#[test]
fn another_open_program_does_not_supply_a_call() {
    let one = "fn helper(a: Int) -> Int { a }\nfn main() -> Int { helper(1) }\n";
    let two = "fn helper(s: Str, t: Str) -> Int { 0 }\nfn main() -> Int { nothing(1) }\n";
    let p = Project::new(&[("one/main.nz", one), ("two/main.nz", two)]);
    let (a, b) = (p.path("one/main.nz"), p.path("two/main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&a, 1, one.into());
        s.open(&b, 1, two.into());
        let h = help(&s, &a, 1, at(one, "1)", 0, 0));
        assert_eq!(shown(&h), ("fn helper(a: Int) -> Int", Some(0)));
        assert_eq!(declared(&s, &a, 1, &h.call), Some(("main.nz".into(), 3)));
        // Same bytes offset, other document: its own compilation, where it is unresolved.
        assert_eq!(
            s.signature_help(&b, 1, at(two, "1)", 0, 0)).unwrap_err(),
            NoCall::NotChecked
        );
    });
}

#[test]
fn a_thousand_edits_leave_one_current_call() {
    let with = "fn f(a: Int, b: Int) -> Int { a }\nfn main() -> Int { f(1, 2) }\n";
    let without = "fn g(a: Int) -> Int { a }\nfn main() -> Int { g(1) + g(2) }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 0, without.into());
        let mut after_warmup = ((0, 0, 0), 0);
        for version in 1..=1000 {
            let (text, label, active) = if version % 2 == 0 {
                (without, "fn g(a: Int) -> Int", Some(0))
            } else {
                (with, "fn f(a: Int, b: Int) -> Int", Some(1))
            };
            s.change(&m, version, text.into());
            let offset = u32::try_from(text.rfind(')').unwrap()).unwrap() - 1;
            assert_eq!(
                shown(&help(&s, &m, version, offset)),
                (label, active),
                "version {version}"
            );
            if version > 1 {
                assert_eq!(
                    s.signature_help(&m, version - 1, offset).unwrap_err(),
                    NoCall::NotCurrent
                );
            }
            if version == 10 {
                after_warmup = (s.retained(), s.calls());
            }
        }
        assert_eq!((s.retained(), s.calls()), after_warmup);
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source.
// ---------------------------------------------------------------------------------------

#[test]
fn signature_help_never_reinterprets_a_call() {
    // Over real source: every call in every file of every clean compilation was recorded
    // by the checker with the callee resolution gave it, the signature the environment
    // gave it and — for a generic callee — exactly the type arguments the backend reads;
    // and the service, asked inside each call of the root file, reports that call.
    use nazm_core::__syntax::SyntaxKind;
    let root = workspace();
    let mut roots = vec![root.join("compiler/emit.nz")];
    for e in walk(&root.join("compiler/conformance/modules")) {
        if e.file_name().is_some_and(|n| n == "main.nz") {
            roots.push(e);
        }
    }
    roots.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let (mut audited, mut generic, mut asked) = (0usize, 0usize, 0usize);
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
            let types = res.types();
            let mut service = LanguageService::new();
            let text = std::fs::read_to_string(path).unwrap();
            service.open(path, 1, text.clone());
            for f in loaded.map.ids() {
                let cst = nazm_core::__syntax::parse_cst(f, loaded.map.text(f)).cst;
                for node in cst.nodes() {
                    if !matches!(node.kind(), SyntaxKind::CallExpr | SyntaxKind::SpawnStmt) {
                        continue;
                    }
                    let callee = node
                        .child_leaves()
                        .into_iter()
                        .find(|l| l.kind == SyntaxKind::Ident)
                        .unwrap()
                        .span;
                    // A conversion (Gate 2) is the language's, not a call: it has no callee and
                    // no signature to reinterpret.
                    if matches!(
                        res.numeric(callee),
                        Some(nazm_core::sema::NumericFact::Convert { .. })
                    ) {
                        assert_eq!(res.callee(callee), None);
                        continue;
                    }
                    let call = res
                        .checked_call(callee)
                        .unwrap_or_else(|| panic!("an unrecorded call at {callee:?}"));
                    assert_eq!(Some(call.callee), res.callee(callee));
                    match call.callee {
                        Callee::Function(id) => {
                            let def = res.function(id).unwrap();
                            assert_eq!(call.params, def.params);
                            assert_eq!(call.ret, def.ret);
                        }
                        Callee::Builtin(i) => {
                            assert_eq!(call.params.len(), i.params().len());
                            for (t, shape) in call.params.iter().zip(i.params()) {
                                // `Ints` and `Strs` are `Vec[Int]` and `Vec[Str]` (R1, 5b.18).
                                if let Some(m) = shape.monomorphic() {
                                    let types = res.types();
                                    let m = match m {
                                        Type::Ints => types.find_vec(Type::Int).unwrap_or(m),
                                        Type::Strs => types.find_vec(Type::Str).unwrap_or(m),
                                        m => m,
                                    };
                                    assert_eq!(*t, m);
                                }
                            }
                        }
                        // Through a function value (N50): the value's type is the signature.
                        Callee::Indirect(t) => {
                            let Type::Fn(f) = t else {
                                panic!("an indirect call through a `{t:?}`");
                            };
                            let sig = res.types().fn_signature(f).unwrap();
                            assert_eq!(call.params, sig.params);
                            assert_eq!(call.ret, sig.ret);
                        }
                    }
                    if let Some(instance) = &call.instance {
                        generic += 1;
                        let settled: Vec<Type> = instance.args.iter().map(|a| a.unwrap()).collect();
                        assert_eq!(Some(settled.as_slice()), res.type_args(callee));
                        for (p, got) in call.params.iter().zip(&instance.params) {
                            assert_eq!(*got, types.substituted(*p, instance.owner, &settled));
                        }
                    } else {
                        assert_eq!(res.type_args(callee), None);
                    }
                    audited += 1;
                    if f == nazm_span::FileId::ROOT {
                        let list = node
                            .children()
                            .into_iter()
                            .find(|c| c.kind() == SyntaxKind::ArgList)
                            .unwrap();
                        let open = list
                            .child_leaves()
                            .into_iter()
                            .find(|l| l.kind == SyntaxKind::LParen)
                            .unwrap();
                        let got = service.call_at(path, 1, open.span.end).unwrap();
                        assert_eq!(got.callee_start, callee.start);
                        assert_eq!(got.callee, call.callee);
                        assert_eq!(got.active_argument, 0);
                        asked += 1;
                    }
                }
            }
        }
        eprintln!("{audited} calls audited, {generic} generic; {asked} asked of the service");
        assert!(
            audited > 5_000 && generic > 0 && asked > 1_000,
            "{audited} {generic} {asked}"
        );
    });
}
