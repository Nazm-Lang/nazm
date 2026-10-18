//! Modules, visibility and imports — the whole rule, through the binary.
//!
//! # Why these cases, and why through the binary
//!
//! `docs/spec.md` settles what a module owns, what `pub` does, what `use` brings in, and
//! what happens when two names collide. Every clause of it is a decision a reader will
//! rely on, so each has a case here, and each runs the real compiler over real files on
//! disk — the loader, the graph, the checker and, where the program is meant to run, the
//! backend as well.
//!
//! Where the answer is a value rather than a diagnostic, it is taken **both ways**:
//! interpreted and compiled, required to agree. A visibility rule the two implementations
//! disagreed about would be a rule the language does not actually have.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).expect("write");
}

/// Run a program interpreted and compiled, and require one answer.
fn both_ways(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        write(dir.path(), name, text);
    }
    let root = dir.path().join(files[0].0);

    let interpreted = Command::new(NAZM)
        .args(["run", "--interpret", root.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        interpreted.status.success(),
        "interpreted: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    let exe = dir.path().join(crate::common::exe_name("prog"));
    let built = Command::new(NAZM)
        .args(["build", root.to_str().unwrap(), "-o", exe.to_str().unwrap()])
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "compiled: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("execute");
    assert!(ran.status.success(), "the compiled program failed");

    let a = String::from_utf8_lossy(&interpreted.stdout)
        .trim()
        .to_owned();
    let b = String::from_utf8_lossy(&ran.stdout).trim().to_owned();
    assert_eq!(a, b, "the two consumers disagree about what a name means");
    a
}

/// Check a program; return its diagnostics and exit code.
fn check(files: &[(&str, &str)]) -> (String, i32) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        write(dir.path(), name, text);
    }
    let out = Command::new(NAZM)
        .args(["check", dir.path().join(files[0].0).to_str().unwrap()])
        .output()
        .expect("check");
    (
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Check and require it to pass.
fn accepted(files: &[(&str, &str)]) {
    let (stderr, code) = check(files);
    assert_eq!(code, 0, "{stderr}");
}

/// Check and require exactly this code, once.
fn refused(files: &[(&str, &str)], code_wanted: &str) -> String {
    let (stderr, code) = check(files);
    assert_eq!(code, 1, "expected a refusal, got:\n{stderr}");
    assert!(
        stderr.contains(code_wanted),
        "expected {code_wanted}, got:\n{stderr}"
    );
    stderr
}

// --- what a module may name ----------------------------------------------------------

#[test]
fn a_module_calls_its_own_private_function() {
    let out = both_ways(&[(
        "main.nz",
        "fn helper() -> Int { 7 }\nfn main() -> Int { helper() }\n",
    )]);
    assert_eq!(out, "7");
}

#[test]
fn a_module_calls_an_imported_public_function() {
    let out = both_ways(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn main() -> Int { twice(3) }\n",
        ),
        ("lib.nz", "pub fn twice(n: Int) -> Int { n + n }\n"),
    ]);
    assert_eq!(out, "6");
}

#[test]
fn a_module_may_not_call_an_imported_private_function() {
    let stderr = refused(
        &[
            (
                "main.nz",
                "use \"lib.nz\";\nfn main() -> Int { secret() }\n",
            ),
            ("lib.nz", "fn secret() -> Int { 1 }\n"),
        ],
        "N0206",
    );
    // The definition is named, in the file the reader just imported. "Not defined" would
    // be true and would send them looking for something that is in front of them.
    assert!(stderr.contains("defined privately here"), "{stderr}");
    assert!(stderr.contains("lib.nz"), "{stderr}");
}

#[test]
fn two_modules_may_each_own_a_private_helper_of_the_same_name() {
    // The case that decides whether these are modules or a naming convention.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn size() -> Int { 1 }\nfn main() -> Int { size() + report() }\n",
        ),
        (
            "lib.nz",
            "pub fn report() -> Int { size() }\nfn size() -> Int { 40 }\n",
        ),
    ]);
    assert_eq!(out, "41");
}

// --- imports are not transitive -------------------------------------------------------

#[test]
fn a_name_reachable_only_through_a_dependency_is_not_in_scope() {
    let stderr = refused(
        &[
            ("a.nz", "use \"b.nz\";\nfn main() -> Int { deep() }\n"),
            ("b.nz", "use \"c.nz\";\npub fn mid() -> Int { deep() }\n"),
            ("c.nz", "pub fn deep() -> Int { 7 }\n"),
        ],
        "N0201",
    );
    // A missing `use` is the overwhelmingly likely cause, so the diagnostic says which
    // module has it rather than only that this one does not.
    assert!(stderr.contains("imports are not transitive"), "{stderr}");
    assert!(stderr.contains("c.nz"), "{stderr}");
}

#[test]
fn importing_it_directly_is_the_repair() {
    let out = both_ways(&[
        (
            "a.nz",
            "use \"b.nz\";\nuse \"c.nz\";\nfn main() -> Int { deep() + mid() }\n",
        ),
        ("b.nz", "use \"c.nz\";\npub fn mid() -> Int { deep() }\n"),
        ("c.nz", "pub fn deep() -> Int { 7 }\n"),
    ]);
    assert_eq!(out, "14");
}

// --- collisions -----------------------------------------------------------------------

#[test]
fn two_imports_exporting_one_name_are_ambiguous() {
    let stderr = refused(
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { same() }\n",
            ),
            ("a.nz", "pub fn same() -> Int { 1 }\n"),
            ("b.nz", "pub fn same() -> Int { 2 }\n"),
        ],
        "N0207",
    );
    // Both exporters, so the reader does not have to find the second one.
    assert!(stderr.contains("also brought in here"), "{stderr}");
}

#[test]
fn an_ambiguity_is_refused_where_it_is_created_not_where_it_is_used() {
    // No mention of `same` anywhere in the program. It is still refused: with no
    // qualified path to write, deferring the error would only move the dead end.
    refused(
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { 0 }\n",
            ),
            ("a.nz", "pub fn same() -> Int { 1 }\n"),
            ("b.nz", "pub fn same() -> Int { 2 }\n"),
        ],
        "N0207",
    );
}

#[test]
fn an_import_that_collides_with_a_local_definition_is_refused() {
    let stderr = refused(
        &[
            (
                "main.nz",
                "use \"lib.nz\";\nfn size() -> Int { 1 }\nfn main() -> Int { size() }\n",
            ),
            ("lib.nz", "pub fn size() -> Int { 2 }\n"),
        ],
        "N0208",
    );
    assert!(stderr.contains("defined here"), "{stderr}");
    assert!(stderr.contains("and exported here"), "{stderr}");
}

#[test]
fn a_private_definition_of_that_name_is_not_a_collision() {
    // Only exports collide. A module keeping a private `size` is not in anyone's way,
    // which is the same rule as `two_modules_may_each_own_a_private_helper`, stated from
    // the importer's side.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn size() -> Int { 1 }\nfn main() -> Int { size() + shown() }\n",
        ),
        (
            "lib.nz",
            "pub fn shown() -> Int { size() }\nfn size() -> Int { 2 }\n",
        ),
    ]);
    assert_eq!(out, "3");
}

#[test]
fn a_definition_may_not_take_a_builtin_name_however_it_is_marked() {
    for keyword in ["fn", "pub fn"] {
        refused(
            &[(
                "main.nz",
                &format!("{keyword} str_len(s: Str) -> Int {{ 0 }}\nfn main() -> Int {{ 0 }}\n"),
            )],
            "N0203",
        );
    }
}

// --- the shape of the graph -----------------------------------------------------------

#[test]
fn the_same_module_reached_two_ways_is_one_module() {
    let out = both_ways(&[
        (
            "main.nz",
            "use \"left.nz\";\nuse \"right.nz\";\nfn main() -> Int { l() + r() }\n",
        ),
        ("left.nz", "use \"shared.nz\";\npub fn l() -> Int { s() }\n"),
        (
            "right.nz",
            "use \"shared.nz\";\npub fn r() -> Int { s() }\n",
        ),
        ("shared.nz", "pub fn s() -> Int { 21 }\n"),
    ]);
    assert_eq!(out, "42");
}

#[test]
fn a_module_may_import_itself_and_nothing_happens() {
    // It already sees all of its own definitions, so the import adds nothing — and in
    // particular does not make the module collide with itself.
    let out = both_ways(&[(
        "main.nz",
        "use \"main.nz\";\npub fn v() -> Int { 3 }\nfn main() -> Int { v() }\n",
    )]);
    assert_eq!(out, "3");
}

#[test]
fn an_import_cycle_still_compiles_and_runs() {
    // Interfaces for the whole graph, then bodies. Neither stage needs an ordering, so
    // there is no ordering for a cycle to violate — and each module calls into the other,
    // so the cycle is load-bearing rather than decorative.
    //
    // The calls themselves do not recurse. A *call* cycle is refused by `nazm build`
    // whatever the module structure (`N0101`, the native subset's bound on recursion),
    // and mixing the two questions into one case would test that limitation instead of
    // this rule.
    let out = both_ways(&[
        (
            "a.nz",
            "use \"b.nz\";\n\
             pub fn one() -> Int { 1 }\n\
             pub fn three() -> Int { 3 }\n\
             fn main() -> Int { two() + three() }\n",
        ),
        (
            "b.nz",
            "use \"a.nz\";\npub fn two() -> Int { one() + one() }\n",
        ),
    ]);
    assert_eq!(out, "5");
}

#[test]
fn a_module_cycle_may_also_be_a_call_cycle_when_it_is_interpreted() {
    // The language allows both, and since N49 so does the native backend. Kept because it
    // is the case a reader would ask about after the test above.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         pub fn down(n: Int) -> Int { if n <= 0 { 0 } else { up(n - 1) } }\n\
         fn main() -> Int { down(6) }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "use \"a.nz\";\npub fn up(n: Int) -> Int { down(n - 1) + 1 }\n",
    );
    let root = dir.path().join("a.nz");
    let out = Command::new(NAZM)
        .args(["run", "--interpret", root.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3");

    let exe = dir.path().join(crate::common::exe_name("prog"));
    let built = Command::new(NAZM)
        .args(["build", root.to_str().unwrap(), "-o", exe.to_str().unwrap()])
        .output()
        .expect("build");
    // Refused natively until N49; compiled since, and the same answer.
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&exe).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "3");
}

#[test]
fn a_public_call_across_a_cycle_resolves_in_both_directions() {
    let out = both_ways(&[
        (
            "a.nz",
            "use \"b.nz\";\npub fn a_value() -> Int { 4 }\nfn main() -> Int { b_value() }\n",
        ),
        (
            "b.nz",
            "use \"a.nz\";\npub fn b_value() -> Int { a_value() * 10 }\n",
        ),
    ]);
    assert_eq!(out, "40");
}

#[test]
fn a_forward_call_within_one_module_still_works() {
    let out = both_ways(&[(
        "main.nz",
        "fn main() -> Int { later() }\nfn later() -> Int { 9 }\n",
    )]);
    assert_eq!(out, "9");
}

// --- the entry point ------------------------------------------------------------------

#[test]
fn an_imported_main_is_an_ordinary_definition_not_a_second_entry_point() {
    // Until N2 this program did not exist: two `main`s in one compilation were a
    // duplicate definition, so a helper file that was also runnable had to rename its
    // entry point.
    let out = both_ways(&[
        ("root.nz", "use \"tool.nz\";\nfn main() -> Int { run() }\n"),
        (
            "tool.nz",
            "pub fn run() -> Int { 5 }\nfn main() -> Int { run() * 100 }\n",
        ),
    ]);
    assert_eq!(out, "5");
}

#[test]
fn a_main_in_an_imported_module_does_not_satisfy_the_root() {
    let (stderr, code) = check(&[
        ("root.nz", "use \"tool.nz\";\npub fn v() -> Int { 1 }\n"),
        ("tool.nz", "fn main() -> Int { 0 }\n"),
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0204"), "{stderr}");
}

#[test]
fn main_need_not_be_public_to_be_the_entry_point() {
    accepted(&[("main.nz", "fn main() -> Int { 0 }\n")]);
    // And marking it `pub` is allowed, and means only that another module could call it.
    accepted(&[("main.nz", "pub fn main() -> Int { 0 }\n")]);
}

// --- record types across modules ------------------------------------------------------
//
// The type namespace follows the same law as the function namespace, and these are the
// same cases asked of it. What is new is that a record has a *shape* as well as a name:
// an importer that can see one can construct it and project its fields, so the whole
// definition crosses the boundary, not just the spelling.

#[test]
fn an_exported_record_is_named_constructed_and_projected_by_an_importer() {
    let out = both_ways(&[
        (
            "main.nz",
            "use \"model.nz\";\n\
             fn size(p: Point) -> Int { p.x + str_len(p.label) }\n\
             fn main() -> Int { size(Point(x: 40, label: \"ab\")) }\n",
        ),
        (
            "model.nz",
            "pub struct Point {\n    x: Int,\n    label: Str,\n}\n",
        ),
    ]);
    assert_eq!(out, "42");
}

#[test]
fn a_record_returned_across_a_module_boundary_keeps_its_owning_field() {
    // The decisive shape: the library's frame exits, and the caller still holds a valid
    // string. Both ways, and through independently generated objects in the compiled one.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"model.nz\";\n\
             fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    \
             while i < 10 {\n        let p = at(i);\n        \
             total = total + str_len(p.label) + p.x;\n        i = i + 1;\n    }\n    total\n}\n",
        ),
        (
            "model.nz",
            "pub struct Point {\n    x: Int,\n    label: Str,\n}\n\
             pub fn at(n: Int) -> Point {\n    Point(x: n, label: str_concat(\"p\", int_to_str(n)))\n}\n",
        ),
    ]);
    assert_eq!(out, "65");
}

#[test]
fn a_private_record_is_not_nameable_from_an_importer() {
    refused(
        &[
            (
                "main.nz",
                "use \"model.nz\";\nfn main() -> Int { let p = Hidden(n: 1); p.n }\n",
            ),
            ("model.nz", "struct Hidden {\n    n: Int,\n}\n"),
        ],
        "N0305",
    );
}

#[test]
fn a_public_function_may_not_return_a_private_record() {
    // Without opaque types there is no honest way to publish this: the importer would
    // have to name the type to use the result and cannot.
    let out = refused(
        &[
            ("main.nz", "use \"model.nz\";\nfn main() -> Int { 0 }\n"),
            (
                "model.nz",
                "struct Secret {\n    value: Int,\n}\n\
                 pub fn get() -> Secret { Secret(value: 1) }\n",
            ),
        ],
        "N0337",
    );
    assert!(out.contains("Secret"), "{out}");
}

#[test]
fn a_public_record_may_not_expose_a_private_one_through_a_field() {
    let out = refused(
        &[
            ("main.nz", "use \"model.nz\";\nfn main() -> Int { 0 }\n"),
            (
                "model.nz",
                "struct Secret {\n    value: Int,\n}\n\
                 pub struct Public {\n    secret: Secret,\n}\n",
            ),
        ],
        "N0337",
    );
    assert!(out.contains("Secret"), "{out}");
}

#[test]
fn a_public_parameter_of_a_private_record_type_is_refused_too() {
    refused(
        &[
            ("main.nz", "use \"model.nz\";\nfn main() -> Int { 0 }\n"),
            (
                "model.nz",
                "struct Secret {\n    value: Int,\n}\n\
                 pub fn take(s: Secret) -> Int { s.value }\n",
            ),
        ],
        "N0337",
    );
}

#[test]
fn two_imported_modules_exporting_one_type_name_is_ambiguous() {
    refused(
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { 0 }\n",
            ),
            ("a.nz", "pub struct User {\n    n: Int,\n}\n"),
            ("b.nz", "pub struct User {\n    m: Int,\n}\n"),
        ],
        "N0207",
    );
}

#[test]
fn an_imported_type_that_collides_with_a_local_one_is_refused() {
    refused(
        &[
            (
                "main.nz",
                "use \"a.nz\";\nstruct User {\n    n: Int,\n}\nfn main() -> Int { 0 }\n",
            ),
            ("a.nz", "pub struct User {\n    m: Int,\n}\n"),
        ],
        "N0208",
    );
}

#[test]
fn the_same_record_name_in_two_modules_is_two_types() {
    // Neither module imports the other, so both may own the name — and they are
    // different types, with different native identities, in one program.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"a.nz\";\nuse \"b.nz\";\n\
             struct Thing {\n    here: Int,\n}\n\
             fn main() -> Int { Thing(here: 1).here + from_a() + from_b() }\n",
        ),
        (
            "a.nz",
            "struct Thing {\n    n: Int,\n}\npub fn from_a() -> Int { Thing(n: 10).n }\n",
        ),
        (
            "b.nz",
            "struct Thing {\n    m: Str,\n}\npub fn from_b() -> Int { str_len(Thing(m: \"abc\").m) }\n",
        ),
    ]);
    assert_eq!(out, "14");
}

#[test]
fn a_module_import_cycle_with_records_is_still_legal() {
    // A module cycle and an inline layout cycle are different things: only the second is
    // refused, and this is the first.
    let out = both_ways(&[
        ("main.nz", "use \"a.nz\";\nfn main() -> Int { via_a() }\n"),
        (
            "a.nz",
            "use \"b.nz\";\n\
             pub struct Ay {\n    n: Int,\n}\n\
             pub fn via_a() -> Int { Ay(n: 2).n * via_b() }\n",
        ),
        (
            "b.nz",
            "use \"a.nz\";\n\
             pub struct Bee {\n    m: Int,\n}\n\
             pub fn via_b() -> Int { Bee(m: 21).m }\n",
        ),
    ]);
    assert_eq!(out, "42");
}

#[test]
fn a_layout_cycle_across_modules_is_refused() {
    refused(
        &[
            ("main.nz", "use \"a.nz\";\nfn main() -> Int { 0 }\n"),
            ("a.nz", "use \"b.nz\";\npub struct Ay {\n    b: Bee,\n}\n"),
            ("b.nz", "use \"a.nz\";\npub struct Bee {\n    a: Ay,\n}\n"),
        ],
        "N0336",
    );
}

// --- enums across a module boundary -----------------------------------------------------
//
// The same law as a record's, with one addition that is the milestone's: an importer that
// can see an enum can *match* it, so what crosses the boundary is every variant and every
// payload shape — and adding a variant is therefore a change to the public type.

#[test]
fn an_exported_enum_is_named_constructed_and_matched_by_an_importer() {
    let out = both_ways(&[
        (
            "main.nz",
            "use \"state.nz\";\n\
             fn weight(s: State) -> Int { match s { State.Ready() => 2, \
             State.Done(code: c) => c, } }\n\
             fn main() -> Int { weight(State.Done(code: 40)) + weight(State.Ready()) }\n",
        ),
        (
            "state.nz",
            "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n",
        ),
    ]);
    assert_eq!(out, "42");
}

#[test]
fn an_enum_returned_across_a_module_boundary_keeps_its_active_payload() {
    // The library's frame exits and the caller still holds a valid string — through
    // independently generated objects in the compiled one, which is where the two sides
    // have to agree about the discriminant as well as the layout.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"state.nz\";\n\
             fn weight(s: State) -> Int { match s { State.Ready() => 0, \
             State.Failed(why: w, code: c) => str_len(w) + c, } }\n\
             fn main() -> Int {\n    let mut total = 0;\n    let mut i = 0;\n    \
             while i < 10 {\n        total = total + weight(failure(i));\n        i = i + 1;\n    }\n    \
             total\n}\n",
        ),
        (
            "state.nz",
            "pub enum State {\n    Ready,\n    Failed(why: Str, code: Int),\n}\n\
             pub fn failure(n: Int) -> State {\n    \
             State.Failed(why: str_concat(\"e\", int_to_str(n)), code: n)\n}\n",
        ),
    ]);
    assert_eq!(out, "65");
}

#[test]
fn a_private_enum_is_not_nameable_from_an_importer() {
    refused(
        &[
            (
                "main.nz",
                "use \"state.nz\";\nfn main() -> Int { match Hidden.A() { Hidden.A() => 1, } }\n",
            ),
            ("state.nz", "enum Hidden {\n    A,\n}\n"),
        ],
        "N0305",
    );
}

#[test]
fn a_public_enum_may_not_expose_a_private_type_through_a_payload() {
    // An importer needs the payload's type to match the variant, and cannot name it.
    refused(
        &[
            ("main.nz", "use \"state.nz\";\nfn main() -> Int { 0 }\n"),
            (
                "state.nz",
                "struct Secret {\n    value: Int,\n}\n\
                 pub enum Public {\n    Hidden(v: Secret),\n}\n",
            ),
        ],
        "N0337",
    );
}

#[test]
fn a_public_record_may_not_expose_a_private_enum_through_a_field() {
    // The closure runs through both kinds, so this is the same refusal one kind over.
    refused(
        &[
            ("main.nz", "use \"state.nz\";\nfn main() -> Int { 0 }\n"),
            (
                "state.nz",
                "enum Secret {\n    A,\n}\n\
                 pub struct Public {\n    s: Secret,\n}\n",
            ),
        ],
        "N0337",
    );
}

#[test]
fn the_same_enum_name_in_two_modules_is_two_types() {
    // Neither imported module exports the name, so all three may own it — and they are
    // different types, with different native identities and different discriminants, in
    // one program.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"a.nz\";\nuse \"b.nz\";\n\
             enum State { Here, }\n\
             fn main() -> Int {\n    \
             match State.Here() { State.Here() => 1, } + from_a() + from_b()\n}\n",
        ),
        (
            "a.nz",
            "enum State { Alpha, Beta(n: Int), }\n\
             pub fn from_a() -> Int { match State.Beta(n: 10) { State.Alpha() => 0, \
             State.Beta(n: n) => n, } }\n",
        ),
        (
            "b.nz",
            "enum State { Only(s: Str), }\n\
             pub fn from_b() -> Int { match State.Only(s: \"abc\") { State.Only(s: s) => str_len(s), } }\n",
        ),
    ]);
    assert_eq!(out, "14");
}

#[test]
fn a_layout_cycle_through_an_enum_across_modules_is_refused() {
    refused(
        &[
            ("main.nz", "use \"a.nz\";\nfn main() -> Int { 0 }\n"),
            ("a.nz", "use \"b.nz\";\npub struct Ay {\n    b: Bee,\n}\n"),
            ("b.nz", "use \"a.nz\";\npub enum Bee {\n    V(a: Ay),\n}\n"),
        ],
        "N0336",
    );
}

#[test]
fn an_enum_payload_reaches_a_third_modules_shape_through_the_interface() {
    // The closure, one kind over: `main` never imports `deep.nz`, cannot name `Inner`,
    // and still has to understand the shape `State.V` hands it in order to match on it.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"state.nz\";\n\
             fn main() -> Int { match make() { State.V(i: i) => i.n, } }\n",
        ),
        (
            "state.nz",
            "use \"deep.nz\";\n\
             pub enum State {\n    V(i: Inner),\n}\n\
             pub fn make() -> State { State.V(i: Inner(n: 42)) }\n",
        ),
        ("deep.nz", "pub struct Inner {\n    n: Int,\n}\n"),
    ]);
    assert_eq!(out, "42");
}
