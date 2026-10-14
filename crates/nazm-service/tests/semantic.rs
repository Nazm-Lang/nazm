//! Semantic identifiers through the language service. N23.
//!
//! Every classified identifier is compared by its exact bytes, its class, its role and the
//! compiler's identity for what it names — never by display kind alone — so that a record
//! and a function both called `Box`, two records' `value`, two enums' `Ready`, a shadowed
//! local and its shadow, or a payload label and the binding written after it, taken for one
//! another fails. Every expected list is written out exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::{BTreeMap, BTreeSet};

use nazm_core::sema::ReferenceTarget;
use nazm_service::{
    LanguageService, NoIdentifiers, Role, SemanticClass, SemanticIdentifier, SemanticTarget,
};

mod common;
use common::{on_the_service_stack, walk, workspace, Project};

/// An identifier as the tests compare it: its text, class and role.
fn shown<'t>(text: &'t str, ids: &[SemanticIdentifier]) -> Vec<(&'t str, SemanticClass, Role)> {
    ids.iter()
        .map(|i| (&text[i.start as usize..i.end as usize], i.class, i.role))
        .collect()
}

/// The identifier that starts at the `n`th occurrence of `needle`.
fn at<'a>(
    text: &str,
    ids: &'a [SemanticIdentifier],
    needle: &str,
    n: usize,
) -> &'a SemanticIdentifier {
    let mut from = 0;
    let mut found = 0;
    for _ in 0..=n {
        found = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} #{n}"))
            + from;
        from = found + needle.len();
    }
    let start = u32::try_from(found).unwrap();
    ids.iter()
        .find(|i| i.start == start)
        .unwrap_or_else(|| panic!("no identifier over {needle:?} #{n}"))
}

use Role::{Declaration as D, Reference as R};
use SemanticClass::{
    BuiltinType, Enum, Field, Function, Intrinsic, Parameter, PayloadField, Record, TypeParameter,
    Variable, Variant,
};

const CATEGORIES: &str = "\
struct Box {
    value: Int,
}
fn Box(value: Int) -> Int { value }
struct Pair[A, B] {
    first: A,
    second: B,
}
enum State {
    Idle,
    Done(code: Int),
}
fn identity[T](x: T) -> T { x }
fn main() -> Int {
    // Point helper State.Idle Box value
    let s = \"Point helper State.Idle Box value\";
    let b = Box(value: 1);
    let n = Box(2);
    let p = Pair[Int, Str](first: 1, second: s);
    let d = State.Done(code: n);
    let x = 1;
    let y = { let x = 2; x };
    match d {
        State.Done(code: code) => code + b.value + x + y + identity(n),
        State.Idle() => str_len(p.second),
    }
}
";

#[test]
fn every_identifier_is_what_the_checker_resolved_it_to() {
    let p = Project::new(&[("main.nz", CATEGORIES)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, CATEGORIES.to_owned());
        assert_eq!(published[0].diagnostics, [], "the fixture checks");
        let found = s.semantic_identifiers(&main, 1).unwrap();
        let ids = &found.identifiers;
        // Nothing from the comment or the string, no keyword, and every name the checker
        // resolved — in source order.
        assert_eq!(
            shown(CATEGORIES, ids),
            [
                ("Box", Record, D),
                ("value", Field, D),
                ("Int", BuiltinType, R),
                ("Box", Function, D),
                ("value", Parameter, D),
                ("Int", BuiltinType, R),
                ("Int", BuiltinType, R),
                ("value", Parameter, R),
                ("Pair", Record, D),
                ("A", TypeParameter, D),
                ("B", TypeParameter, D),
                ("first", Field, D),
                ("A", TypeParameter, R),
                ("second", Field, D),
                ("B", TypeParameter, R),
                ("State", Enum, D),
                ("Idle", Variant, D),
                ("Done", Variant, D),
                ("code", PayloadField, D),
                ("Int", BuiltinType, R),
                ("identity", Function, D),
                ("T", TypeParameter, D),
                ("x", Parameter, D),
                ("T", TypeParameter, R),
                ("T", TypeParameter, R),
                ("x", Parameter, R),
                ("main", Function, D),
                ("Int", BuiltinType, R),
                ("s", Variable, D),
                ("b", Variable, D),
                ("Box", Record, R),
                ("value", Field, R),
                ("n", Variable, D),
                ("Box", Function, R),
                ("p", Variable, D),
                ("Pair", Record, R),
                ("Int", BuiltinType, R),
                ("Str", BuiltinType, R),
                ("first", Field, R),
                ("second", Field, R),
                ("s", Variable, R),
                ("d", Variable, D),
                ("State", Enum, R),
                ("Done", Variant, R),
                ("code", PayloadField, R),
                ("n", Variable, R),
                ("x", Variable, D),
                ("y", Variable, D),
                ("x", Variable, D),
                ("x", Variable, R),
                ("d", Variable, R),
                ("State", Enum, R),
                ("Done", Variant, R),
                ("code", PayloadField, R),
                ("code", Variable, D),
                ("code", Variable, R),
                ("b", Variable, R),
                ("value", Field, R),
                ("x", Variable, R),
                ("y", Variable, R),
                ("identity", Function, R),
                ("n", Variable, R),
                ("State", Enum, R),
                ("Idle", Variant, R),
                ("str_len", Intrinsic, R),
                ("p", Variable, R),
                ("second", Field, R),
            ]
        );
        // Strictly ordered, never overlapping.
        for w in ids.windows(2) {
            assert!(w[0].end <= w[1].start, "{:?} then {:?}", w[0], w[1]);
        }
    });
}

#[test]
fn a_declaration_and_its_uses_are_one_identity_and_same_spellings_are_not() {
    let p = Project::new(&[("main.nz", CATEGORIES)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, CATEGORIES.to_owned());
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        let t = CATEGORIES;
        let same = |a: &SemanticIdentifier, b: &SemanticIdentifier| {
            assert_eq!((a.target, a.class), (b.target, b.class), "{a:?} {b:?}");
            assert_eq!((a.role, b.role), (D, R));
        };
        // The record `Box` and its construction; the function `Box` and its call.
        same(at(t, &ids, "Box", 0), at(t, &ids, "Box", 4));
        same(at(t, &ids, "Box", 1), at(t, &ids, "Box", 5));
        assert_ne!(at(t, &ids, "Box", 0).target, at(t, &ids, "Box", 1).target);
        // The field `value`, its label and its projection; the parameter `value` and its use.
        // (Occurrences 3 and 4 are in the comment and the string, and are nothing.)
        same(at(t, &ids, "value", 0), at(t, &ids, "value", 5));
        same(at(t, &ids, "value", 0), at(t, &ids, "value", 6));
        same(at(t, &ids, "value", 1), at(t, &ids, "value", 2));
        assert_ne!(
            at(t, &ids, "value", 0).target,
            at(t, &ids, "value", 1).target
        );
        // A type parameter and its uses; two generic definitions' parameters differ.
        same(at(t, &ids, "A", 0), at(t, &ids, "A", 1));
        same(at(t, &ids, "T", 0), at(t, &ids, "T", 1));
        assert_ne!(at(t, &ids, "A", 0).target, at(t, &ids, "T", 0).target);
        // `State.Done(code: code) => code`: the label is the payload field, the binding a
        // new local, and the arm's `code` that local.
        let label = at(t, &ids, "code: code", 0);
        same(at(t, &ids, "code", 0), label);
        let binding = at(t, &ids, "code", 3);
        let arm = at(t, &ids, "code", 4);
        same(binding, arm);
        assert_eq!(binding.class, Variable);
        assert_ne!(label.target, binding.target);
        // The outer `x` and the inner one: two locals, each use its own.
        let outer = at(t, &ids, "x = 1", 0);
        let inner = at(t, &ids, "x = 2", 0);
        assert_ne!(outer.target, inner.target);
        same(inner, at(t, &ids, "x };", 0));
        same(outer, at(t, &ids, "x + y", 0));
        // A parameter named like a field is a parameter; a local named like a parameter
        // elsewhere is a variable.
        assert_eq!(at(t, &ids, "value", 1).class, Parameter);
        assert!(matches!(
            at(t, &ids, "value", 1).target,
            SemanticTarget::Entity(ReferenceTarget::Local(_))
        ));
        // A compiler-owned name carries the compiler's identity, not an entity's.
        assert!(matches!(
            at(t, &ids, "str_len", 0).target,
            SemanticTarget::Intrinsic(_)
        ));
        assert_eq!(
            at(t, &ids, "Int", 0).target,
            at(t, &ids, "Int", 1).target,
            "one built-in type"
        );
        assert_ne!(at(t, &ids, "Int", 0).target, at(t, &ids, "Str", 0).target);
    });
}

#[test]
fn the_out_main_is_given_is_used_and_never_declared_in_source() {
    // R1, 5b.1: a `main` that writes no `OutCap` or `IoCap` holds `out: OutCap` at a span of
    // no width. A use of it is a parameter's use; nothing was written where it is declared, so
    // nothing there is an identifier, a definition to go to, or a name to rename — and the
    // `Int` a function written with no result type is given (5b.2) is no written type either.
    const T: &str = "fn say(out: OutCap) {\n    println(\"x\")\n}\nfn main() {\n    say(out)\n}\n";
    let p = Project::new(&[("main.nz", T)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, T.to_owned());
        assert_eq!(published[0].diagnostics, [], "the fixture checks");
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        assert!(ids.iter().all(|i| i.end > i.start), "{ids:?}");
        assert_eq!(
            shown(T, &ids),
            [
                ("say", Function, D),
                ("out", Parameter, D),
                ("OutCap", BuiltinType, R),
                ("println", Intrinsic, R),
                ("main", Function, D),
                ("say", Function, R),
                ("out", Parameter, R),
            ]
        );
        let used = at(T, &ids, "out", 1);
        assert_ne!(used.target, at(T, &ids, "out", 0).target, "two parameters");
        let offset = used.start;
        assert_eq!(s.definition(&main, 1, offset), None);
        let r = s.references(&main, 1, offset).expect("an entity");
        assert_eq!((r.kind, r.declaration.as_ref()), ("local", None));
        assert_eq!(
            r.uses.iter().map(|l| (l.start, l.end)).collect::<Vec<_>>(),
            [(offset, offset + 3)]
        );
        assert_eq!(
            s.prepare_rename(&main, 1, offset).unwrap_err(),
            nazm_service::Refusal::ToolchainOwned
        );
    });
}

#[test]
fn same_spellings_in_two_records_and_two_enums_are_two_identities() {
    const TEXT: &str = "struct A {\n    value: Int,\n}\nstruct B {\n    value: Str,\n}\n\
                        enum EA {\n    Ready,\n}\nenum EB {\n    Ready,\n}\n\
                        fn f(a: A, b: B) -> Int { let r = EA.Ready(); let q = EB.Ready(); a.value + str_len(b.value) }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, TEXT.to_owned());
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        let target = |needle: &str, n: usize| at(TEXT, &ids, needle, n).target;
        assert_ne!(target("value", 0), target("value", 1));
        assert_eq!(target("value", 0), target("value", 2), "a.value is A's");
        assert_eq!(target("value", 1), target("value", 3), "b.value is B's");
        assert_ne!(target("Ready", 0), target("Ready", 1));
        assert_eq!(target("Ready", 0), target("Ready", 2));
        assert_eq!(target("Ready", 1), target("Ready", 3));
        assert_eq!(
            at(TEXT, &ids, "EA", 1).class,
            Enum,
            "the qualifier is the enum"
        );
        assert_eq!(
            at(TEXT, &ids, "Ready", 2).class,
            Variant,
            "and the variant its own token"
        );
    });
}

#[test]
fn what_did_not_resolve_is_not_classified_wherever_it_is_written() {
    // An unknown call head is no function, an unknown member no field, an unknown type no
    // type, an unknown qualifier no enum — however they are spelled or placed. What did
    // resolve around them still is.
    const TEXT: &str = "struct P {\n    x: Int,\n}\nfn f(p: P, c: UnknownType) -> Int { let a = unknown(1); let b = p.nope; let d = Nowhere.Thing(); p.x }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, TEXT.to_owned());
        assert!(!published[0].diagnostics.is_empty());
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        let texts: Vec<&str> = shown(TEXT, &ids).into_iter().map(|(t, _, _)| t).collect();
        for absent in ["unknown", "nope", "UnknownType", "Nowhere", "Thing"] {
            assert!(
                !texts.contains(&absent),
                "{absent} was classified: {texts:?}"
            );
        }
        assert_eq!(at(TEXT, &ids, "x }", 0).class, Field, "p.x still resolves");
        assert_eq!(at(TEXT, &ids, "p.x", 0).class, Parameter);
    });
}

#[test]
fn a_recovery_hides_only_what_it_left_unresolved_and_nothing_old_comes_back() {
    const CLEAN: &str =
        "fn helper(n: Int) -> Int { n }\nfn broken() -> Int { let v = helper(1); v }\n";
    // The second body does not parse: its names are resolved nowhere. The first function is
    // untouched and keeps every identifier.
    const BROKEN: &str =
        "fn helper(n: Int) -> Int { n }\nfn broken() -> Int { let v = helper(1; v }\n";
    // `helper` renamed at its declaration: the old use no longer resolves.
    const RENAMED: &str =
        "fn assist(n: Int) -> Int { n }\nfn broken() -> Int { let v = helper(1); v }\n";
    let p = Project::new(&[("main.nz", CLEAN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, CLEAN.to_owned());
        let names =
            |s: &LanguageService, v: i32, text: &str| -> Vec<(String, SemanticClass, Role)> {
                shown(text, &s.semantic_identifiers(&main, v).unwrap().identifiers)
                    .into_iter()
                    .map(|(t, c, r)| (t.to_owned(), c, r))
                    .collect()
            };
        let own = |v: &[(&str, SemanticClass, Role)]| -> Vec<(String, SemanticClass, Role)> {
            v.iter()
                .map(|(t, c, r)| ((*t).to_owned(), *c, *r))
                .collect()
        };
        let clean = own(&[
            ("helper", Function, D),
            ("n", Parameter, D),
            ("Int", BuiltinType, R),
            ("Int", BuiltinType, R),
            ("n", Parameter, R),
            ("broken", Function, D),
            ("Int", BuiltinType, R),
            ("v", Variable, D),
            ("helper", Function, R),
            ("v", Variable, R),
        ]);
        assert_eq!(names(&s, 1, CLEAN), clean);
        s.change(&main, 2, BROKEN.to_owned());
        assert_eq!(
            names(&s, 2, BROKEN),
            own(&[
                ("helper", Function, D),
                ("n", Parameter, D),
                ("Int", BuiltinType, R),
                ("Int", BuiltinType, R),
                ("n", Parameter, R),
                ("broken", Function, D),
                ("Int", BuiltinType, R),
            ])
        );
        assert_eq!(
            s.semantic_identifiers(&main, 1),
            Err(NoIdentifiers::NotCurrent)
        );
        s.change(&main, 3, RENAMED.to_owned());
        assert_eq!(
            names(&s, 3, RENAMED),
            own(&[
                ("assist", Function, D),
                ("n", Parameter, D),
                ("Int", BuiltinType, R),
                ("Int", BuiltinType, R),
                ("n", Parameter, R),
                ("broken", Function, D),
                ("Int", BuiltinType, R),
                ("v", Variable, D),
                ("v", Variable, R),
            ]),
            "the old `helper` use is gone, not remembered"
        );
        s.change(&main, 4, CLEAN.to_owned());
        assert_eq!(names(&s, 4, CLEAN), clean, "repaired, it is back");
    });
}

#[test]
fn a_refused_duplicate_lends_its_type_parameters_to_no_one() {
    // The second `twice` is refused, so its signature was resolved under an id the next
    // function receives: its `[T]` is no declaration, and `after`'s `U` is `after`'s. Its
    // body is still checked — as the first `twice`'s, which is how the checker has always
    // treated a refused duplicate — so its names are what the checker made them: `x` a
    // binding of the first function after its parameter, `T` the first function's `T`.
    const TEXT: &str = "fn twice[T](x: T) -> T { x }\nfn twice[T](x: T) -> T { x }\nfn str_len[V](z: V) -> V { z }\nfn after[U](y: U) -> U { y }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, TEXT.to_owned());
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        let line = |n: usize| -> Vec<(&str, SemanticClass, Role)> {
            let start = TEXT.lines().take(n).map(|l| l.len() + 1).sum::<usize>();
            let end = start + TEXT.lines().nth(n).unwrap().len();
            shown(TEXT, &ids)
                .into_iter()
                .zip(&ids)
                .filter(|(_, i)| (i.start as usize) >= start && (i.end as usize) <= end)
                .map(|(s, _)| s)
                .collect()
        };
        assert_eq!(
            line(0),
            [
                ("twice", Function, D),
                ("T", TypeParameter, D),
                ("x", Parameter, D),
                ("T", TypeParameter, R),
                ("T", TypeParameter, R),
                ("x", Parameter, R),
            ]
        );
        assert_eq!(
            line(1),
            [
                ("x", Variable, D),
                ("T", TypeParameter, R),
                ("T", TypeParameter, R),
                ("x", Variable, R),
            ],
            "the refused `twice` declares nothing of its own"
        );
        // (Occurrence 3 is the refused `[T]`, which is no identifier.)
        assert_eq!(at(TEXT, &ids, "T", 4).target, at(TEXT, &ids, "T", 0).target);
        // A built-in's name cannot be redefined, so this `str_len` was never declared, and
        // its body is checked under no function: its `V` is resolved nowhere, and is not
        // the next function's `U`, whose id it was resolved under.
        let refused = line(2);
        assert!(refused.iter().all(|(t, _, _)| *t != "V"), "{refused:?}");
        assert_eq!(
            line(3),
            [
                ("after", Function, D),
                ("U", TypeParameter, D),
                ("y", Parameter, D),
                ("U", TypeParameter, R),
                ("U", TypeParameter, R),
                ("y", Parameter, R),
            ]
        );
        let u = at(TEXT, &ids, "U", 0).target;
        for n in 1..3 {
            assert_eq!(at(TEXT, &ids, "U", n).target, u);
        }
    });
}

#[test]
fn identifiers_follow_the_buffers_their_own_compilation_and_close() {
    const LIB: &str = "pub fn tool() -> Int { 1 }\npub struct Thing {\n    v: Int,\n}\n";
    const LIB_EDITED: &str = "pub struct tool {\n    v: Int,\n}\n";
    const MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { tool() }\n";
    // Another program, declaring its own `tool` as a record: it must not colour `main.nz`.
    const OTHER: &str = "struct tool {\n    v: Int,\n}\nfn other(t: tool) -> Int { t.v }\n";
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN), ("other.nz", OTHER)]);
    let (lib, main, other) = (p.path("lib.nz"), p.path("main.nz"), p.path("other.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.to_owned());
        s.open(&other, 1, OTHER.to_owned());
        let tool = |s: &LanguageService| {
            let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
            ids.iter()
                .find(|i| &MAIN[i.start as usize..i.end as usize] == "tool")
                .map(|i| (i.class, i.role))
        };
        assert_eq!(
            tool(&s),
            Some((Function, R)),
            "an imported function is a function"
        );
        // An unsaved edit to the dependency: `tool` is now a record, so the call no longer
        // resolves and is not classified at all.
        s.open(&lib, 1, LIB_EDITED.to_owned());
        assert_eq!(tool(&s), None);
        // Closing the dependency makes the disk authoritative again.
        s.close(&lib);
        assert_eq!(tool(&s), Some((Function, R)));
        // Declarations in the dependency are not this document's.
        let ids = s.semantic_identifiers(&main, 1).unwrap().identifiers;
        assert_eq!(
            shown(MAIN, &ids),
            [
                ("main", Function, D),
                ("Int", BuiltinType, R),
                ("tool", Function, R)
            ]
        );
    });
}

#[test]
fn a_thousand_edits_leave_one_current_classification() {
    const ONE: &str = "struct Box {\n    v: Int,\n}\nfn f[T](x: T) -> T { let y = x; y }\n";
    const TWO: &str = "fn Box(v: Int) -> Int { let v = 1; v }\nfn g() -> Int { Box(ghost) }\n";
    let p = Project::new(&[("main.nz", ONE)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 0, ONE.to_owned());
        let baseline = (s.retained(), s.indexed(), s.scoped(), s.calls());
        let one = shown(ONE, &s.semantic_identifiers(&main, 0).unwrap().identifiers);
        let two_expected = [
            ("Box", Function, D),
            ("v", Parameter, D),
            ("Int", BuiltinType, R),
            ("Int", BuiltinType, R),
            ("v", Variable, D),
            ("v", Variable, R),
            ("g", Function, D),
            ("Int", BuiltinType, R),
            ("Box", Function, R),
        ];
        for v in 1..=1000 {
            let text = if v % 2 == 0 { ONE } else { TWO };
            s.change(&main, v, text.to_owned());
            let got = s.semantic_identifiers(&main, v).unwrap().identifiers;
            if v % 2 == 0 {
                assert_eq!(shown(ONE, &got), one, "version {v}");
                assert_eq!(
                    (s.retained(), s.indexed(), s.scoped(), s.calls()),
                    baseline,
                    "version {v}"
                );
            } else {
                assert_eq!(shown(TWO, &got), two_expected, "version {v}");
            }
            assert_eq!(
                s.semantic_identifiers(&main, v - 1),
                Err(NoIdentifiers::NotCurrent)
            );
        }
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source.
// ---------------------------------------------------------------------------------------

#[test]
fn every_resolved_occurrence_of_the_repository_is_one_identifier() {
    // Over real source, in every file that checks cleanly:
    // - every occurrence the reference index holds in the file — declaration or use — is
    //   exactly one identifier, over the same span, naming the same entity, declaring
    //   exactly when the index records the entity declared there;
    // - every N22 symbol's selection is a declaring identifier of the same entity;
    // - every local's use names the local declared where its declaration identifier is;
    // - every written built-in and type-parameter name the checker recorded in the file is
    //   exactly one identifier of that class;
    // - and there is no identifier any of those did not account for.
    use nazm_core::sema::WrittenType;
    use nazm_span::{FileId, Span};
    let root = workspace();
    let mut files = walk(&root.join("compiler"));
    files.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let (mut entities, mut declarations, mut builtins, mut params, mut intrinsics) =
            (0, 0, 0, 0, 0);
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
            let ids = s.semantic_identifiers(path, 1).unwrap().identifiers;
            let by_span: BTreeMap<(u32, u32), &SemanticIdentifier> =
                ids.iter().map(|i| ((i.start, i.end), i)).collect();
            assert_eq!(
                by_span.len(),
                ids.len(),
                "a span twice in {}",
                path.display()
            );
            for w in ids.windows(2) {
                assert!(w[0].end <= w[1].start, "overlap in {}", path.display());
            }
            let mut accounted = BTreeSet::new();
            let span = |i: &SemanticIdentifier| Span::new(FileId::ROOT, i.start, i.end);
            for (target, uses) in index.iter() {
                let declared = index.declaration(target).filter(|d| d.file == FileId::ROOT);
                for (at, role) in declared.iter().map(|d| (*d, D)).chain(
                    uses.iter()
                        .filter(|u| u.file == FileId::ROOT)
                        .map(|u| (*u, R)),
                ) {
                    let id = by_span
                        .get(&(at.start, at.end))
                        .unwrap_or_else(|| panic!("{target:?} at {at:?} in {}", path.display()));
                    assert_eq!(
                        id.target,
                        SemanticTarget::Entity(target),
                        "{}",
                        path.display()
                    );
                    assert_eq!(id.role, role, "{target:?} at {at:?} in {}", path.display());
                    accounted.insert((at.start, at.end));
                    entities += 1;
                }
            }
            let symbols = s.document_symbols(path, 1).unwrap();
            let mut stack: Vec<&nazm_service::Symbol> = symbols.symbols.iter().collect();
            while let Some(symbol) = stack.pop() {
                let id = by_span[&(symbol.name_start, symbol.name_end)];
                assert_eq!(id.target, SemanticTarget::Entity(symbol.target));
                assert_eq!(id.role, D);
                declarations += 1;
                stack.extend(symbol.children.iter());
            }
            for id in &ids {
                if let SemanticTarget::Entity(ReferenceTarget::Local(l)) = id.target {
                    let declared = index.declaration(ReferenceTarget::Local(l)).unwrap();
                    let d = by_span[&(declared.start, declared.end)];
                    assert_eq!(d.target, id.target);
                    assert_eq!(d.role, D);
                }
                if let SemanticTarget::Intrinsic(_) = id.target {
                    assert_eq!(id.class, Intrinsic);
                    accounted.insert((id.start, id.end));
                    intrinsics += 1;
                }
                assert_eq!(
                    index.target(span(id)).is_some(),
                    matches!(id.target, SemanticTarget::Entity(_))
                );
            }
            for (at, what) in a.resolution.written_types() {
                if at.file != FileId::ROOT {
                    continue;
                }
                let id = by_span
                    .get(&(at.start, at.end))
                    .unwrap_or_else(|| panic!("{what:?} at {at:?} in {}", path.display()));
                match what {
                    WrittenType::Builtin(t) => {
                        assert_eq!(
                            (id.class, id.target),
                            (BuiltinType, SemanticTarget::BuiltinType(t))
                        );
                        builtins += 1;
                    }
                    WrittenType::Param(p) => {
                        assert_eq!(
                            (id.class, id.role, id.target),
                            (TypeParameter, R, SemanticTarget::TypeParameter(p))
                        );
                        params += 1;
                    }
                    WrittenType::ParamDeclaration(p) => {
                        assert_eq!(
                            (id.class, id.role, id.target),
                            (TypeParameter, D, SemanticTarget::TypeParameter(p))
                        );
                        params += 1;
                    }
                }
                accounted.insert((at.start, at.end));
            }
            let unaccounted: Vec<_> = ids
                .iter()
                .filter(|i| !accounted.contains(&(i.start, i.end)))
                .collect();
            assert_eq!(
                unaccounted,
                Vec::<&SemanticIdentifier>::new(),
                "{}",
                path.display()
            );
            documents += 1;
        }
        eprintln!(
            "{documents} documents: entity occurrences {entities}, symbol declarations {declarations}, built-in type names {builtins}, type-parameter names {params}, intrinsic calls {intrinsics}"
        );
        assert!(entities > 0 && declarations > 0 && builtins > 0 && intrinsics > 0);
    });
}

#[test]
fn every_written_type_name_of_the_repository_is_classified() {
    // Independently of the checker's new record: in every clean file, every identifier the
    // concrete tree places in a written type is classified as a record, an enum, a built-in
    // type or a type parameter — so no written position was left unrecorded — and every
    // name in a type-parameter list is a type-parameter declaration.
    use nazm_core::__syntax::{CstNode, SyntaxKind};
    let root = workspace();
    let mut files = walk(&root.join("compiler"));
    files.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let mut checked = 0;
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
            let text = std::fs::read_to_string(path).unwrap();
            let mut s = LanguageService::new();
            s.open(path, 1, text.clone());
            let ids = s.semantic_identifiers(path, 1).unwrap().identifiers;
            let class: BTreeMap<(u32, u32), (SemanticClass, Role)> = ids
                .iter()
                .map(|i| ((i.start, i.end), (i.class, i.role)))
                .collect();
            fn visit(n: CstNode<'_>, out: &mut Vec<(SyntaxKind, (u32, u32))>) {
                let names = n
                    .child_leaves()
                    .into_iter()
                    .filter(|l| l.kind == SyntaxKind::Ident);
                match n.kind() {
                    SyntaxKind::Type => {
                        out.extend(names.map(|l| (n.kind(), (l.span.start, l.span.end))));
                    }
                    // The parameter's name; a `: Equality` after it names a capability.
                    SyntaxKind::TypeParam => {
                        out.extend(
                            names
                                .take(1)
                                .map(|l| (n.kind(), (l.span.start, l.span.end))),
                        );
                    }
                    _ => {}
                }
                for c in n.children() {
                    visit(c, out);
                }
            }
            let cst = nazm_core::__syntax::parse_cst(nazm_span::FileId::ROOT, &text).cst;
            let mut written = Vec::new();
            visit(cst.root(), &mut written);
            for (kind, at) in written {
                let got = class.get(&at).copied();
                let ok = match kind {
                    SyntaxKind::TypeParam => got == Some((TypeParameter, D)),
                    // A trait named where a type is written — `impl Show for P`, a bound —
                    // is a trait (N78).
                    _ => matches!(
                        got,
                        Some((
                            Record | Enum | BuiltinType | TypeParameter | SemanticClass::Trait,
                            R
                        ))
                    ),
                };
                assert!(ok, "{kind:?} at {at:?} in {}: {got:?}", path.display());
                checked += 1;
            }
        }
        eprintln!("{checked} written type names");
        assert!(checked > 0);
    });
}
