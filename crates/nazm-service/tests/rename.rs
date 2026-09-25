//! Rename, driven through the language service directly: no protocol. N18.
//!
//! A rename is a validated plan built from the entity's recorded occurrences, never from a
//! text search. Each test states the expected result as the exact edited source, so an
//! edit to anything but the renamed entity — however it is spelled — fails it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::{LanguageService, Refusal, RenamePlan, Target};

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
    fn disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
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

/// The one file a plan edits, as the plan would leave it — applied here, in the test, from
/// the plan's own edits, after checking each edit's precondition.
fn applied(plan: &RenamePlan) -> String {
    assert_eq!(plan.files.len(), 1, "{plan:#?}");
    let f = &plan.files[0];
    let mut out = f.text.to_string();
    for e in f.edits.iter().rev() {
        assert_eq!(&f.text[e.start as usize..e.end as usize], e.expected);
        out.replace_range(e.start as usize..e.end as usize, &e.replacement);
    }
    out
}

/// `text` with the name at each of `offsets` (each `old.len()` bytes) replaced by `new`:
/// the expected result, stated by position.
fn replaced_at(text: &str, offsets: &[u32], old: &str, new: &str) -> String {
    let mut sorted = offsets.to_vec();
    sorted.sort_unstable();
    let mut out = text.to_owned();
    for o in sorted.iter().rev() {
        let o = *o as usize;
        assert_eq!(
            &text[o..o + old.len()],
            old,
            "the expectation is wrong at {o}"
        );
        out.replace_range(o..o + old.len(), new);
    }
    out
}

/// Every occurrence of `needle` in `text`, plus `into`: only for fixtures in which every
/// such occurrence is, by construction, the entity being renamed.
fn all(text: &str, needle: &str, into: usize) -> Vec<u32> {
    text.match_indices(needle)
        .map(|(i, _)| u32::try_from(i + into).unwrap())
        .collect()
}

/// Open `text` as `m.nz` alone and rename the name at `offset` to `new`.
fn rename_in(text: &'static str, offset: u32, new: &'static str) -> Result<RenamePlan, Refusal> {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, text.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let plan = s.rename(&m, 1, offset, new);
        if let Ok(plan) = &plan {
            assert!(s.plan_is_current(plan));
            // The renamed program checks, and the old spelling of this entity is gone.
            let after = applied(plan);
            let out = s.change(&m, 2, after);
            assert!(out[0].diagnostics.is_empty(), "{out:#?}");
            assert!(
                !s.plan_is_current(plan),
                "a plan does not outlive its version"
            );
        }
        plan
    })
}

// ---------------------------------------------------------------------------------------
// Types: every semantic spelling, and nothing else.
// ---------------------------------------------------------------------------------------

const TYPES: &str = "struct Box[T] { value: T, }\n\
struct Point { x: Int, y: Int, }\n\
enum State { Idle, Busy(at: Point), }\n\
struct Holder { p: Point, s: State, b: Box[Point], }\n\
fn make(p: Point) -> Box[Point] { Box[Point](value: p) }\n\
fn nested(b: Box[Box[Point]]) -> Point { b.value.value }\n\
fn step(s: State) -> State { match s { State.Idle() => State.Busy(at: Point(x: 1, y: 2)), State.Busy(at: p) => State.Busy(at: p), } }\n\
fn main() -> Int { let b = make(Point(x: 1, y: 2)); let h = Holder(p: b.value, s: step(State.Idle()), b: b); h.p.x }\n";

#[test]
fn a_type_rename_changes_every_semantic_spelling_and_nothing_else() {
    // In this fixture every `Point`, `State`, `Box` and `value` is the record, enum or
    // field of that name — in a declaration, a parameter, a return, a record field's type,
    // a payload's type, a type argument, a nested type argument, a construction, a
    // qualifier of a construction and of a pattern — so the expected text is every one of
    // them replaced, and nothing else.
    for (what, needle, old, new) in [
        (
            "a record: declaration, construction, and every type position",
            "Point",
            "Point",
            "Pos",
        ),
        (
            "an enum: declaration, types, and every qualifier",
            "State",
            "State",
            "Phase",
        ),
        (
            "a generic record, nested in its own argument",
            "Box",
            "Box",
            "Crate",
        ),
        (
            "a field of a generic record, through nested instances",
            "value",
            "value",
            "item",
        ),
    ] {
        let plan = rename_in(TYPES, at(TYPES, needle, 0, 0), new)
            .unwrap_or_else(|r| panic!("{what}: refused: {r}"));
        assert_eq!(plan.old_name, old);
        assert_eq!(
            applied(&plan),
            replaced_at(TYPES, &all(TYPES, old, 0), old, new),
            "{what}"
        );
    }
}

#[test]
fn a_rename_from_any_occurrence_is_the_same_rename() {
    let from_declaration = rename_in(TYPES, at(TYPES, "State", 0, 0), "Phase").unwrap();
    for n in 1..TYPES.matches("State").count() {
        let from_use = rename_in(TYPES, at(TYPES, "State", n, 0), "Phase").unwrap();
        assert_eq!(
            from_use.files[0].edits, from_declaration.files[0].edits,
            "#{n}"
        );
    }
}

#[test]
fn a_type_annotation_and_an_enum_qualifier_now_go_to_their_definitions() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, TYPES.into());
        let go = |o: u32| {
            let d = s.definition(&m, 1, o).unwrap();
            let Target::File(l) = d.target else { panic!() };
            (d.kind, l.start)
        };
        let point = at(TYPES, "struct Point", 0, 7);
        let state = at(TYPES, "enum State", 0, 5);
        assert_eq!(
            go(at(TYPES, "(p: Point)", 0, 4)),
            ("record", point),
            "a parameter's type"
        );
        assert_eq!(
            go(at(TYPES, "Box[Point]", 0, 4)),
            ("record", point),
            "a type argument"
        );
        assert_eq!(
            go(at(TYPES, "-> State", 0, 3)),
            ("enum", state),
            "a return type"
        );
        // `State.Idle()`: the qualifier is the enum, the name after `.` the variant.
        let q = at(TYPES, "State.Idle()", 0, 0);
        assert_eq!(go(q), ("enum", state), "the qualifier");
        assert_eq!(go(q + 6).0, "variant", "the variant beside it");
        assert_eq!(
            s.references(&m, 1, point).unwrap().uses.len(),
            TYPES.matches("Point").count() - 1,
            "every use of `Point`, all of them written types or constructions"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Only the target changes.
// ---------------------------------------------------------------------------------------

const MIXED: &str = "struct A { value: Int, }\n\
struct B { value: Int, }\n\
enum E { Ready, Done(code: Int), }\n\
enum F { Ready, }\n\
struct Point { x: Int, }\n\
fn Point() -> Int { 1 }\n\
fn f(value: Int) -> Int {\n    \
    let a = A(value: value);\n    \
    let b = B(value: a.value);\n    \
    let inner = { let value = b.value; value + 1 };\n    \
    value + inner\n\
}\n\
fn g(e: E) -> Int { match e { E.Ready() => 0, E.Done(code: code) => code, } }\n\
fn h(x: F) -> Int { match x { F.Ready() => 1, } }\n\
fn main() -> Int { f(1) + g(E.Done(code: 2)) + g(E.Ready()) + h(F.Ready()) + Point() + Point(x: 3).x }\n";

#[test]
fn rename_preserves_semantic_partitions() {
    // Each rename is stated as the exact positions that change. Same-spelled locals in
    // nested scopes, fields of two records, variants of two enums, and a record and a
    // function that share a name: every other occurrence is byte-for-byte untouched.
    let m = |needle: &str, n: usize, into: usize| at(MIXED, needle, n, into);
    let cases: Vec<(&str, u32, &str, &str, Vec<u32>)> = vec![
        (
            "`A.value`, not `B.value` or any local",
            m("A { value", 0, 4),
            "value",
            "amount",
            vec![m("A { value", 0, 4), m("A(value", 0, 2), m("a.value", 0, 2)],
        ),
        (
            "the parameter `value`, not the inner `let value`",
            m("f(value", 0, 2),
            "value",
            "n",
            vec![
                m("f(value", 0, 2),
                m("value: value)", 0, 7),
                m("value + inner", 0, 0),
            ],
        ),
        (
            "the inner `let value`",
            m("let value", 0, 4),
            "value",
            "v",
            vec![m("let value", 0, 4), m("value + 1", 0, 0)],
        ),
        (
            "`E.Ready`, not `F.Ready`",
            m("Ready,", 0, 0),
            "Ready",
            "Waiting",
            vec![
                m("Ready,", 0, 0),
                m("E.Ready()", 0, 2),
                m("E.Ready()", 1, 2),
            ],
        ),
        (
            "the record `Point`, not the function",
            m("struct Point", 0, 7),
            "Point",
            "Spot",
            vec![m("struct Point", 0, 7), m("Point(x: 3)", 0, 0)],
        ),
        (
            "the function `Point`, not the record",
            m("fn Point", 0, 3),
            "Point",
            "one",
            vec![m("fn Point", 0, 3), m("Point() +", 0, 0)],
        ),
    ];
    for (what, from, old, new, expected) in cases {
        let plan = rename_in(MIXED, from, new).unwrap_or_else(|r| panic!("{what}: {r}"));
        assert_eq!(
            applied(&plan),
            replaced_at(MIXED, &expected, old, new),
            "{what}"
        );
    }
}

#[test]
fn an_enum_its_variant_its_payload_field_and_a_pattern_local_rename_apart() {
    // Four things spelled `Code`/`code`: the enum, its variant, the variant's payload field,
    // and the local a pattern binds the field to.
    const T: &str = "enum Code { Code(code: Int), Other, }\n\
fn get(c: Code) -> Int { match c { Code.Code(code: code) => code, Code.Other() => 0, } }\n\
fn main() -> Int { get(Code.Code(code: 7)) }\n";
    let t = |needle: &str, n: usize, into: usize| at(T, needle, n, into);
    for (what, from, old, new, expected) in [
        (
            "the enum",
            t("enum Code", 0, 5),
            "Code",
            "Kind",
            vec![
                t("enum Code", 0, 5),
                t("c: Code", 0, 3),
                t("Code.Code(code: code)", 0, 0),
                t("Code.Other", 0, 0),
                t("Code.Code(code: 7)", 0, 0),
            ],
        ),
        (
            "the variant",
            t("{ Code(", 0, 2),
            "Code",
            "Coded",
            vec![
                t("{ Code(", 0, 2),
                t("Code.Code(code: code)", 0, 5),
                t("Code.Code(code: 7)", 0, 5),
            ],
        ),
        (
            "the payload field, and not the local bound to it",
            t("(code: Int)", 0, 1),
            "code",
            "status",
            vec![
                t("(code: Int)", 0, 1),
                t("(code: code)", 0, 1),
                t("(code: 7)", 0, 1),
            ],
        ),
        (
            "the pattern's local, and not the field it binds",
            t("code) =>", 0, 0),
            "code",
            "result",
            vec![t("code) =>", 0, 0), t("=> code", 0, 3)],
        ),
    ] {
        let plan = rename_in(T, from, new).unwrap_or_else(|r| panic!("{what}: {r}"));
        assert_eq!(
            applied(&plan),
            replaced_at(T, &expected, old, new),
            "{what}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// What the checker and the partition refuse.
// ---------------------------------------------------------------------------------------

const LOCALS: &str = "fn f(x: Int) -> Int {\n    let y = x;\n    let w = 2;\n    if true {\n        let z = y;\n        z + y + w\n    } else {\n        y\n    }\n}\nfn main() -> Int { f(1) }\n";

#[test]
fn a_rename_that_would_capture_or_collide_is_refused_and_a_safe_one_is_not() {
    let l = |needle: &str, n: usize, into: usize| at(LOCALS, needle, n, into);
    // `z` → `y` makes `z + y` read `y + y`: the second `y`, the outer binding, would be
    // captured by the inner one. The program still checks; its meaning does not survive.
    assert_eq!(
        rename_in(LOCALS, l("let z", 0, 4), "y").unwrap_err(),
        Refusal::MeaningChanged,
        "capture"
    );
    // `w` → `y` puts two `let y` in one scope, which the checker refuses.
    assert!(
        matches!(
            rename_in(LOCALS, l("let w", 0, 4), "y"),
            Err(Refusal::Rejected(_))
        ),
        "collision in one scope"
    );
    // `y` → `x` shadows the parameter in a nested scope — legal, and every use still
    // refers to what it did: accepted, with exactly `y`'s occurrences changed.
    let plan = rename_in(LOCALS, l("let y", 0, 4), "x").unwrap();
    assert_eq!(
        applied(&plan),
        replaced_at(
            LOCALS,
            &[
                l("let y", 0, 4),
                l("= y;", 0, 2),
                l("+ y +", 0, 2),
                l("        y\n", 0, 8)
            ],
            "y",
            "x"
        )
    );
    // And a plain one.
    let plan = rename_in(LOCALS, l("let z", 0, 4), "inner").unwrap();
    assert_eq!(
        applied(&plan),
        replaced_at(LOCALS, &[l("let z", 0, 4), l("z + y", 0, 0)], "z", "inner")
    );
}

#[test]
fn a_function_rename_that_collides_or_takes_a_built_ins_name_is_refused() {
    const T: &str = "fn a() -> Int { 1 }\nfn b() -> Int { 2 }\nfn main() -> Int { a() + b() }\n";
    assert!(
        matches!(rename_in(T, 3, "b"), Err(Refusal::Rejected(_))),
        "another function"
    );
    assert!(
        matches!(rename_in(T, 3, "str_len"), Err(Refusal::Rejected(_))),
        "a built-in"
    );
    assert!(rename_in(T, 3, "first").is_ok());
    // A record renamed to a built-in type's name, or to another record's.
    const R: &str = "struct P { x: Int, }\nstruct Q { y: Int, }\nfn f(p: P) -> Int { p.x }\nfn main() -> Int { f(P(x: 1)) + Q(y: 2).y }\n";
    assert!(rename_in(R, 7, "Int").is_err(), "a built-in type");
    assert!(rename_in(R, 7, "Q").is_err(), "another record");
}

#[test]
fn a_new_name_must_be_one_identifier() {
    const T: &str = "fn a() -> Int { 1 }\nfn main() -> Int { a() }\n";
    for bad in [
        "",
        " ",
        "two words",
        "a-b",
        "a.b",
        "9a",
        "fn",
        "match",
        "struct",
        "_",
        "a(",
        "é",
    ] {
        let r = rename_in(T, 3, bad);
        assert!(
            matches!(r, Err(Refusal::InvalidName(_) | Refusal::Rejected(_))),
            "{bad:?}: {r:?}"
        );
    }
    assert_eq!(rename_in(T, 3, "a").unwrap_err(), Refusal::Unchanged);
}

#[test]
fn what_is_not_a_programs_own_entity_is_not_renamed() {
    const T: &str = "fn id[T](x: T) -> T { x }\n\
fn main() -> Int {\n    let s = \"a\"; // a comment\n    let r = Result[Int, Int].Ok(value: str_len(s));\n    match r { Result.Ok(value: v) => id[Int](v), Result.Err(error: e) => e, }\n}\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, T.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let refused = |o: u32| s.prepare_rename(&m, 1, o).unwrap_err();
        assert_eq!(
            refused(at(T, "str_len", 0, 1)),
            Refusal::NoEntity,
            "a built-in"
        );
        assert_eq!(
            refused(at(T, "-> Int {\n", 0, 4)),
            Refusal::NoEntity,
            "a built-in type"
        );
        assert_eq!(
            refused(at(T, "[T]", 0, 1)),
            Refusal::NoEntity,
            "a type parameter"
        );
        assert_eq!(
            refused(at(T, "a comment", 0, 2)),
            Refusal::NoEntity,
            "a comment"
        );
        assert_eq!(refused(at(T, "\"a\"", 0, 1)), Refusal::NoEntity, "a string");
        assert_eq!(
            refused(at(T, "  let s", 0, 0)),
            Refusal::NoEntity,
            "whitespace"
        );
        assert_eq!(
            refused(u32::try_from(T.len()).unwrap()),
            Refusal::NoEntity,
            "the end"
        );
        assert_eq!(
            refused(at(T, "Ok(value: str", 0, 0)),
            Refusal::ToolchainOwned,
            "a prelude variant"
        );
        assert_eq!(
            refused(at(T, "Result.Ok", 0, 0)),
            Refusal::ToolchainOwned,
            "a prelude enum"
        );
        assert_eq!(
            refused(at(T, "value: v", 0, 0)),
            Refusal::ToolchainOwned,
            "a prelude payload field"
        );
        // What is the program's own is renameable.
        let r = s.prepare_rename(&m, 1, at(T, "fn id", 0, 3)).unwrap();
        assert_eq!((r.kind, r.name.as_str()), ("function", "id"));
        assert!(s.prepare_rename(&m, 1, at(T, "let s", 0, 4)).is_ok());
        assert!(
            s.prepare_rename(&m, 1, at(T, "=> e", 0, 3)).is_ok(),
            "a pattern local"
        );
        assert_eq!(
            s.prepare_rename(&m, 2, at(T, "fn id", 0, 3)).unwrap_err(),
            Refusal::NotCurrent
        );
    });
}

#[test]
fn a_program_with_errors_offers_no_rename() {
    // The error is elsewhere in the file, but a resolution with an error in it is not a
    // complete account of the file's uses, so nothing in it is renamed.
    const T: &str = "fn helper() -> Int { 1 }\nfn main() -> Int { helper() }\nfn broken() -> Int { let x = ; helper() }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, T.into());
        assert!(!out[0].diagnostics.is_empty());
        assert_eq!(s.prepare_rename(&m, 1, 3).unwrap_err(), Refusal::NotClean);
        assert_eq!(
            s.rename(&m, 1, 3, "renamed").unwrap_err(),
            Refusal::NotClean
        );
    });
}

// ---------------------------------------------------------------------------------------
// Scope: locals and private definitions are complete; exported ones are not claimed.
// ---------------------------------------------------------------------------------------

const LIB: &str = "pub fn shared(n: Int) -> Int { helper(n) }\nfn helper(n: Int) -> Int { n + 1 }\npub struct Pt { x: Int, }\nstruct Hidden { y: Int, }\nfn hide() -> Int { Hidden(y: 1).y }\n";
const OTHER: &str = "pub fn more() -> Int { helper() }\nfn helper() -> Int { 2 }\n";
const MAIN: &str =
    "use \"lib.nz\";\nuse \"other.nz\";\nfn main() -> Int { shared(1) + more() + Pt(x: 2).x }\n";

#[test]
fn a_private_definition_is_renamed_in_its_module_and_an_exported_one_is_refused() {
    let p = Project::new(&[("lib.nz", LIB), ("other.nz", OTHER), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        s.open(&lib, 1, LIB.into());
        // `lib.nz`'s private `helper`: its uses are all in `lib.nz`, and `other.nz`'s own
        // private `helper` is untouched. Validated in both compilations that load `lib.nz`.
        let plan = s
            .rename(&lib, 1, at(LIB, "fn helper", 0, 3), "assist")
            .unwrap();
        assert_eq!(
            applied(&plan),
            replaced_at(
                LIB,
                &[at(LIB, "fn helper", 0, 3), at(LIB, "helper(n)", 0, 0)],
                "helper",
                "assist"
            )
        );
        assert_eq!(
            plan.files[0].version,
            Some(1),
            "an open document, at its version"
        );
        // A private record and its field.
        assert!(s
            .rename(&lib, 1, at(LIB, "struct Hidden", 0, 7), "Secret")
            .is_ok());
        assert!(s.rename(&lib, 1, at(LIB, "{ y", 0, 2), "why").is_ok());
        // Exported: a function, a record, the record's field — refused, from the
        // declaration and from an importer's use alike.
        for (path, text, needle, into) in [
            (&lib, LIB, "pub fn shared", 7),
            (&main, MAIN, "shared(1)", 0),
            (&lib, LIB, "struct Pt", 7),
            (&main, MAIN, "Pt(x", 0),
            (&main, MAIN, ".x", 1),
        ] {
            assert_eq!(
                s.prepare_rename(path, 1, at(text, needle, 0, into))
                    .unwrap_err(),
                Refusal::Exported,
                "{needle}"
            );
        }
        // Nothing was written.
        assert_eq!(p.disk("lib.nz"), LIB);
        assert_eq!(p.disk("main.nz"), MAIN);
    });
}

#[test]
fn a_private_rename_that_would_break_an_importers_program_is_refused() {
    // Renaming `lib.nz`'s private `helper` to `shared` collides with its own public
    // `shared` — and the importer's compilation, which also loads `lib.nz`, is checked too.
    let p = Project::new(&[("lib.nz", LIB), ("other.nz", OTHER), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        s.open(&lib, 1, LIB.into());
        assert!(matches!(
            s.rename(&lib, 1, at(LIB, "fn helper", 0, 3), "shared"),
            Err(Refusal::Rejected(_))
        ));
    });
}

// ---------------------------------------------------------------------------------------
// Stale-safe.
// ---------------------------------------------------------------------------------------

#[test]
fn a_plan_is_current_only_for_the_exact_text_and_version_it_was_made_from() {
    const T: &str = "fn a() -> Int { 1 }\nfn main() -> Int { a() }\n";
    let p = Project::new(&[("other.nz", "fn main() -> Int { 0 }\n")]);
    let (m, other) = (p.path("m.nz"), p.path("other.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, T.into());
        let plan = s.rename(&m, 1, 3, "b").unwrap();
        assert!(s.plan_is_current(&plan));
        // Every edit states the bytes it replaces.
        for e in &plan.files[0].edits {
            assert_eq!(e.expected, "a");
            assert_eq!(&plan.files[0].text[e.start as usize..e.end as usize], "a");
        }
        // Any mutation of the service moves its generation: another document opening is
        // enough, since it may import this one.
        s.open(&other, 1, "fn main() -> Int { 0 }\n".into());
        assert!(!s.plan_is_current(&plan));
        let plan = s.rename(&m, 1, 3, "b").unwrap();
        assert!(s.plan_is_current(&plan));
        // An edit to the document itself, even back to the same text at a new version.
        s.change(&m, 2, T.into());
        assert!(!s.plan_is_current(&plan));
        assert_eq!(s.rename(&m, 1, 3, "b").unwrap_err(), Refusal::NotCurrent);
        // A plan whose recorded text is not the document's is not current either — even
        // when every edited range still reads what it expects, as it does here: the text
        // differs only in a byte no edit touches, and an offset is only meaningful in the
        // whole text it was computed from.
        let mut forged = s.rename(&m, 2, 3, "b").unwrap();
        assert!(s.plan_is_current(&forged));
        forged.files[0].text = T.replacen("{ 1 }", "{ 2 }", 1).into();
        assert!(forged.files[0]
            .edits
            .iter()
            .all(|e| forged.files[0].text[e.start as usize..e.end as usize] == e.expected));
        assert!(!s.plan_is_current(&forged));
    });
}

#[test]
fn a_thousand_rename_plans_leave_no_state_behind() {
    const T: &str = "fn a() -> Int { 1 }\nfn main() -> Int { a() + a() }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 0, T.into());
        let before = (s.retained(), s.indexed());
        for version in 1..=1000 {
            if version % 2 == 0 {
                s.change(&m, version, T.into());
            }
            let v = if version % 2 == 0 {
                version
            } else {
                version - 1
            };
            assert!(s.prepare_rename(&m, v, 3).is_ok());
            let plan = s.rename(&m, v, 3, "b").unwrap();
            assert_eq!(plan.files[0].edits.len(), 3);
        }
        // Planning and validating keep nothing: one document, one analysis, one index.
        assert_eq!((s.retained(), s.indexed()), before);
    });
}
