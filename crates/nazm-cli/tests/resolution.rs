//! One resolution, two consumers — asserted where they could disagree.
//!
//! # Why these cases
//!
//! Name resolution used to happen twice: once in `nazm_core::check` and again in
//! `nazm_lir::lower`, which rebuilt an equivalent scope stack and slot allocator because
//! it could not see the first. They agreed, and the comments in the backend said they
//! agreed *deliberately* — "reproducing that is the point; inventing a shadowing rule here
//! would make the two disagree."
//!
//! Each test below is a program whose meaning depends on a resolution decision, run
//! **interpreted and compiled**, and required to give the same answer. Before N1 that
//! compared two implementations of one rule. Now it confirms one implementation reaches
//! both — and a divergence would mean the backend had stopped reading the resolution,
//! which is a different and more interesting failure than the old drift.

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

/// Run a program both ways and require one answer.
fn both_ways(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        write(dir.path(), name, text);
    }
    let root = dir.path().join(files[0].0);

    let interpreted = Command::new(NAZM)
        .args(["run", root.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        interpreted.status.success(),
        "interpreted: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    let exe = dir.path().join("prog");
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

/// Check a program and return its diagnostics as text.
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

#[test]
fn a_shadowing_binding_is_a_different_slot_in_both() {
    // The case where one slot allocator would be visibly wrong: the inner `x` must not
    // overwrite the outer one, so the answer is 1 and not 2.
    let out = both_ways(&[(
        "main.nz",
        "fn main() -> Int {\n\
         \x20   let x = 1;\n\
         \x20   if true { let x = 2; x } else { 0 };\n\
         \x20   x\n\
         }\n",
    )]);
    assert_eq!(out, "1");
}

#[test]
fn a_parameter_shadowed_by_a_let_keeps_the_parameters_slot_live() {
    // `let x = x + 1` reads the parameter and binds a new one. Sharing storage would
    // read the value being written.
    let out = both_ways(&[(
        "main.nz",
        "fn f(x: Int) -> Int { let x = x + 1; x }\nfn main() -> Int { f(41) }\n",
    )]);
    assert_eq!(out, "42");
}

#[test]
fn a_forward_reference_resolves_in_both() {
    let out = both_ways(&[(
        "main.nz",
        "fn main() -> Int { later() }\nfn later() -> Int { 7 }\n",
    )]);
    assert_eq!(out, "7");
}

#[test]
fn a_call_into_an_imported_file_resolves_in_both() {
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
fn a_diamond_import_resolves_to_one_definition_in_both() {
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
fn a_local_does_not_shadow_a_function_of_the_same_name() {
    // Functions live in their own namespace. A local `f` must not make `f()` mean the
    // local — which is the rule the backend used to restate for itself.
    let out = both_ways(&[(
        "main.nz",
        "fn f() -> Int { 5 }\nfn main() -> Int { let f = 1; f() + f }\n",
    )]);
    assert_eq!(out, "6");
}

#[test]
fn a_builtin_resolves_before_a_user_function_could() {
    // Built-ins share the ordinary function namespace and are resolved first. Both
    // consumers read that decision rather than each applying the rule.
    let out = both_ways(&[("main.nz", "fn main() -> Int { str_len(\"abcd\") }\n")]);
    assert_eq!(out, "4");
}

#[test]
fn redefining_a_builtin_is_refused_by_name() {
    let (stderr, code) = check(&[(
        "main.nz",
        "fn str_len(s: Str) -> Int { 0 }\nfn main() -> Int { 1 }\n",
    )]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0203"), "{stderr}");
    assert!(stderr.contains("built-in"), "{stderr}");
}

#[test]
fn two_functions_with_one_name_are_refused_once() {
    let (stderr, code) = check(&[(
        "main.nz",
        "fn g() -> Int { 1 }\nfn g() -> Int { 2 }\nfn main() -> Int { g() }\n",
    )]);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(
        stderr.matches("N0203").count(),
        1,
        "reported twice: {stderr}"
    );
}

#[test]
fn two_lets_of_one_name_in_one_scope_are_refused() {
    let (stderr, code) = check(&[("main.nz", "fn main() -> Int { let x = 1; let x = 2; x }\n")]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0202"), "{stderr}");
}

#[test]
fn two_modules_may_each_own_a_private_function_of_the_same_name() {
    // Changed by N2, 2026-09-22. This was `a_duplicate_function_across_files_is_still_one
    // _conflict`, and it was right about the language it was written for: every top-level
    // name was in one namespace, so `dup` in two files was `N0203`. Owning your names is
    // the whole of what a module is, and a module system in which this failed would be a
    // naming convention.
    let out = both_ways(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn dup() -> Int { 1 }\nfn main() -> Int { dup() + twice() }\n",
        ),
        (
            "lib.nz",
            "pub fn twice() -> Int { dup() * 2 }\nfn dup() -> Int { 20 }\n",
        ),
    ]);
    // 1 from the root's `dup`, 40 from lib's — which is the point: `dup()` in lib.nz
    // means lib.nz's, and the root's call means the root's.
    assert_eq!(out, "41");
}

#[test]
fn a_duplicate_function_within_one_module_is_still_one_conflict() {
    let (stderr, code) = check(&[(
        "main.nz",
        "fn dup() -> Int { 1 }\nfn dup() -> Int { 2 }\nfn main() -> Int { dup() }\n",
    )]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0203"), "{stderr}");
}
