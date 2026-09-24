//! N12: the core prelude as a module, and `Result` across module, interface and cache
//! boundaries.
//!
//! The prelude is one more module in every compilation: `@core/prelude`, whose identity
//! the toolchain fixes, imported by every other module with no `use`. These tests hold the
//! consequences `docs/spec.md` and `docs/architecture.md` §7.13 claim for that design:
//!
//! * its durable identity and its published interface are the **same bytes** in every
//!   project, from every directory — so `Result` in one project is `Result` in another;
//! * a dependent can check `?` over a dependency's `Result` API from persisted interfaces
//!   alone, with the dependency's source deleted, and still tell the core `Result` from a
//!   lookalike;
//! * changing a body from `match` to `?` moves no interface, and changing an exported error
//!   type does — and a change to the prelude's own public shape reaches every module;
//! * the object cache is untouched by any of it: a warm build compiles nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

use nazm_core::sema::interface::UnitInterface;
use nazm_core::sema::key::ModuleKey;
use nazm_core::sema::module::{ModuleGraph, ModuleId};
use nazm_core::sema::resolve::{FnId, Resolution};
use nazm_core::sema::udt::UserTypes;
use nazm_iface::{InterfaceHash, PersistentUnitInterface, Rehydrated};
use nazm_span::{FileId, SourceMap};
use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

fn nazm(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(NAZM)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run nazm")
}

/// The stored check entry whose own module is `key`.
fn entry_for(dir: &Path, key: &str) -> Option<String> {
    let wanted = format!(r#""module":"{key}","source""#);
    let root = dir.join(".nazm/check");
    for bucket in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        for entry in std::fs::read_dir(bucket.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if text.contains(&wanted) {
                    return Some(text);
                }
            }
        }
    }
    None
}

/// `nazm check` with a cache report: `(accepted, checked, reused)` and the codes raised.
fn check(dir: &Path, file: &str) -> ((bool, u64, u64), Vec<String>) {
    let out = nazm(dir, &["check", file, "--cache-report", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: Value = stdout
        .lines()
        .find(|l| l.contains("nazm.check-report/1"))
        .map(|l| serde_json::from_str(l).expect("a report"))
        .expect("a cache report");
    let codes = String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect();
    (
        (
            out.status.success(),
            report["checked"].as_u64().unwrap(),
            report["reused"].as_u64().unwrap(),
        ),
        codes,
    )
}

fn interface_of(dir: &Path, file: &str) -> String {
    let out = nazm(dir, &["interface", file]);
    assert!(
        out.status.success(),
        "nazm interface refused {file}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// `(compiled, reused)` for modules and for generic instances, from a build report.
fn build(dir: &Path, root: &str) -> (u64, u64, u64, u64) {
    let out = nazm(dir, &["build", root, "-o", "prog", "--build-report"]);
    assert!(
        out.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = stdout
        .lines()
        .find(|l| l.contains("nazm.build-report/1"))
        .map(|l| serde_json::from_str(l).expect("a report"))
        .expect("a build report");
    let n = |a: &Value, k: &str| a[k].as_u64().unwrap();
    (
        n(&v["native"], "compiled"),
        n(&v["native"], "reused"),
        n(&v["native"]["instances"], "compiled"),
        n(&v["native"]["instances"], "reused"),
    )
}

// --- one identity, everywhere --------------------------------------------------------------

#[test]
fn the_prelude_is_the_same_bytes_in_every_project_from_every_directory() {
    // Two unrelated projects, in two directories, with different entry points, checked
    // from different working directories. The prelude's stored entry — its key, its
    // source fingerprint, its dependencies and its published interface — is one file's
    // worth of bytes in both, and names neither directory.
    let one = tempfile::tempdir().expect("tempdir");
    let two = tempfile::tempdir().expect("tempdir");
    write(one.path(), "nazm.root", "");
    write(one.path(), "main.nz", "fn main() -> Int { 1 }\n");
    write(two.path(), "nazm.root", "");
    write(
        two.path(),
        "deep/app.nz",
        "fn f() -> Result[Int, Str] { Result[Int, Str].Ok(value: 2) }\nfn main() -> Int { 0 }\n",
    );

    assert!(check(one.path(), "main.nz").0 .0);
    // From the parent of the project, naming the file by a longer path.
    let parent = two.path().parent().unwrap();
    let named = two.path().join("deep/app.nz");
    let out = nazm(parent, &["check", named.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let a = entry_for(one.path(), "@core/prelude").expect("the prelude's entry, project one");
    let b = entry_for(two.path(), "@core/prelude").expect("the prelude's entry, project two");
    assert_eq!(a, b, "the prelude is not the same module in two projects");
    for dir in [one.path(), two.path(), parent] {
        let shown = dir.to_str().unwrap();
        assert!(!a.contains(shown), "the entry names a directory: {a}");
    }
    assert!(
        a.contains(r#""dependencies":[]"#),
        "the prelude imports nothing: {a}"
    );
}

#[test]
fn a_project_file_under_a_directory_named_core_is_not_the_prelude() {
    // `@core` is the toolchain's. A project file that happens to sit under a directory of
    // that name compiles, has no durable identity, and is never mistaken for the prelude.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "nazm.root", "");
    write(dir.path(), "@core/prelude.nz", "pub fn v() -> Int { 3 }\n");
    write(
        dir.path(),
        "main.nz",
        "use \"@core/prelude.nz\";\nfn main() -> Int { v() }\n",
    );
    let ((ok, _, _), codes) = check(dir.path(), "main.nz");
    assert!(ok, "{codes:?}");
    let out = nazm(dir.path(), &["run", "main.nz"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3");
}

// --- `Result` across a module boundary --------------------------------------------------------

const B: &str = "pub struct ParseError {\n    at: Int,\n}\n\
    pub fn parse(s: Str) -> Result[Int, ParseError] {\n    let mut i = 0;\n    let mut n = 0;\n    \
    while i < str_len(s) {\n        let d = str_byte(s, i) - 48;\n        \
    if d < 0 {\n            return Result[Int, ParseError].Err(error: ParseError(at: i));\n        }\n        \
    if d > 9 {\n            return Result[Int, ParseError].Err(error: ParseError(at: i));\n        }\n        \
    n = n * 10 + d;\n        i = i + 1;\n    }\n    Result[Int, ParseError].Ok(value: n)\n}\n";

const A: &str = "use \"b.nz\";\n\
    fn sum(x: Str, y: Str) -> Result[Int, ParseError] {\n    Result[Int, ParseError].Ok(value: parse(x)? + parse(y)?)\n}\n\
    fn main() -> Int {\n    let good = match sum(\"40\", \"2\") {\n        Result.Ok(value: n) => n,\n        \
    Result.Err(error: e) => 0 - 1,\n    };\n    let bad = match sum(\"4\", \"x2\") {\n        \
    Result.Ok(value: n) => 0 - 1,\n        Result.Err(error: e) => e.at,\n    };\n    good * 10 + bad\n}\n";

fn a_and_b(dir: &Path, b: &str) {
    write(dir, "nazm.root", "");
    write(dir, "a.nz", A);
    write(dir, "b.nz", b);
}

#[test]
fn a_dependency_s_result_propagates_in_its_importer() {
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    let interpreted = nazm(dir.path(), &["run", "a.nz"]);
    assert_eq!(String::from_utf8_lossy(&interpreted.stdout).trim(), "420");
    build(dir.path(), "a.nz");
    let native = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&native.stdout).trim(), "420");
}

/// The prelude's published interface, computed the way the checker's driver computes it.
fn prelude_interface() -> String {
    struct Capture(Option<String>);
    impl nazm_core::Reuse for Capture {
        fn plan(&mut self, graph: &ModuleGraph, interfaces: &[UnitInterface], types: &UserTypes) {
            let core = graph.prelude().expect("a prelude");
            let published = interfaces
                .get(core.index())
                .and_then(|i| PersistentUnitInterface::published(i, graph, types).ok())
                .expect("the prelude publishes an interface");
            self.0 = Some(published.to_json().expect("json"));
        }
        fn must_check(&self, _: ModuleId) -> bool {
            true
        }
        fn checked_clean(&mut self, _: ModuleId) {}
    }
    let (map, graph) = nazm_core::one_file("fn main() -> Int { 0 }\n");
    let (program, errors) = nazm_core::__syntax::parse_map(&map);
    assert!(errors.is_empty(), "{errors:#?}");
    let mut capture = Capture(None);
    let (_, errors) = nazm_core::check_reusing(&program, &graph, &mut capture, &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    capture.0.expect("captured")
}

/// Check `a.nz`'s source against B's and the prelude's persisted interfaces, and nothing
/// else. `prelude_first` chooses the order they are rehydrated in, which must not matter.
fn check_against(
    a_source: &str,
    b_json: &str,
    core_json: &str,
    prelude_first: bool,
) -> Vec<nazm_diag::Diagnostic> {
    let map = SourceMap::from_parts(vec![("a.nz".to_owned(), a_source.to_owned())]);
    let (program, parse_errors) = nazm_core::__syntax::parse_map(&map);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let b_iface = PersistentUnitInterface::from_json(b_json).expect("B's interface");
    let core_iface = PersistentUnitInterface::from_json(core_json).expect("the prelude's");
    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(FileId::from_index(1), b_iface.module_key());
    graph.import(a, b, nazm_span::Span::synthetic());
    let core = graph.attach_prelude(FileId::from_index(2));
    assert_eq!(core_iface.module_key(), Some(ModuleKey::prelude()));

    let units = [nazm_core::Unit::of(&program, &graph, a)];
    let mut res = Resolution::default();
    let mut errors = Vec::new();
    let mut seen = Rehydrated::default();
    let (b_side, core_side) = if prelude_first {
        let c = core_iface.rehydrate_into(core, FnId::from_index(8192), res.types_mut(), &mut seen);
        let bb = b_iface.rehydrate_into(b, FnId::from_index(4096), res.types_mut(), &mut seen);
        (bb, c)
    } else {
        let bb = b_iface.rehydrate_into(b, FnId::from_index(4096), res.types_mut(), &mut seen);
        let c = core_iface.rehydrate_into(core, FnId::from_index(8192), res.types_mut(), &mut seen);
        (bb, c)
    };
    nazm_core::declare_types(&units, &mut res, &mut errors);
    nazm_core::declare_unit(&units[0], &[&b_side, &core_side], &mut res, &mut errors);
    nazm_core::check_unit(&units[0], &[&b_side, &core_side], &mut res, &mut errors);
    errors
}

#[test]
fn a_dependent_checks_propagation_from_persisted_interfaces_with_the_dependency_deleted() {
    // §91. B's interface and the prelude's, and no B source anywhere: A's `?` over
    // `parse`'s `Result[Int, ParseError]` is checked against both, in either order.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    let b_json = interface_of(dir.path(), "b.nz");
    assert!(
        b_json.contains("@core/prelude::enum Result"),
        "B's interface should name the core `Result` by its durable identity: {b_json}"
    );
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    let core_json = prelude_interface();

    for prelude_first in [true, false] {
        let errors = check_against(A, &b_json, &core_json, prelude_first);
        assert!(
            errors.is_empty(),
            "A should check from the two interfaces alone (prelude first: {prelude_first}): {errors:#?}"
        );
    }

    // The negative halves, so the pass above is not something else succeeding.
    //
    // 1. `parse` now fails with a `Str`: A's `?` inside a `Result[Int, ParseError]`
    //    function propagates the wrong error type.
    let wrong_error = b_json.replace(
        r#""ret":{"def":"@core/prelude::enum Result","args":["Int",{"def":"b.nz::record ParseError","args":[]}]}"#,
        r#""ret":{"def":"@core/prelude::enum Result","args":["Int","Str"]}"#,
    );
    assert_ne!(wrong_error, b_json, "the interface was not changed");
    let errors = check_against(A, &wrong_error, &core_json, true);
    assert!(
        errors
            .iter()
            .any(|d| d.code == nazm_diag::codes::PROPAGATE_ERROR_MISMATCH),
        "{errors:#?}"
    );

    // 2. The same shape under a project module's identity: B claims its own `Result`.
    //    Everything about it matches except the definition, which is what `?` checks.
    let lookalike = b_json.replace("@core/prelude::enum Result", "b.nz::enum Result");
    assert_ne!(lookalike, b_json);
    for prelude_first in [true, false] {
        let errors = check_against(A, &lookalike, &core_json, prelude_first);
        assert!(
            errors
                .iter()
                .any(|d| d.code == nazm_diag::codes::PROPAGATE_NON_RESULT),
            "prelude first: {prelude_first}: {errors:#?}"
        );
    }
}

#[test]
fn every_module_s_check_entry_depends_on_the_prelude_s_interface() {
    // §11: the implicit import is a dependency like any other, so a module's stored entry
    // names the prelude's key and the exact interface hash it was checked against — which
    // is what makes a change to the prelude's public shape reach it.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    assert!(check(dir.path(), "a.nz").0 .0);
    let core = entry_for(dir.path(), "@core/prelude").expect("the prelude's entry");
    let hash = serde_json::from_str::<Value>(&core).expect("json")["interface_hash"]
        .as_str()
        .expect("a hash")
        .to_owned();
    for module in ["a.nz", "b.nz"] {
        let entry = entry_for(dir.path(), module).expect("an entry");
        let wanted = format!(r#"{{"module":"@core/prelude","interface":"{hash}"}}"#);
        assert!(
            entry.contains(&wanted),
            "{module}'s entry does not depend on the prelude's interface: {entry}"
        );
    }
}

// --- what moves an interface, and what does not ---------------------------------------------

const B_BY_MATCH: &str = "pub struct ParseError {\n    at: Int,\n}\n\
    fn digit(s: Str, i: Int) -> Result[Int, ParseError] {\n    let d = str_byte(s, i) - 48;\n    \
    if d < 0 { return Result[Int, ParseError].Err(error: ParseError(at: i)); }\n    \
    if d > 9 { return Result[Int, ParseError].Err(error: ParseError(at: i)); }\n    \
    Result[Int, ParseError].Ok(value: d)\n}\n\
    pub fn parse(s: Str) -> Result[Int, ParseError] {\n    let mut i = 0;\n    let mut n = 0;\n    \
    while i < str_len(s) {\n        let d = match digit(s, i) {\n            Result.Ok(value: d) => d,\n            \
    Result.Err(error: e) => if true { return Result[Int, ParseError].Err(error: e); } \
    else { return Result[Int, ParseError].Err(error: e); },\n        };\n        \
    n = n * 10 + d;\n        i = i + 1;\n    }\n    Result[Int, ParseError].Ok(value: n)\n}\n";

fn b_by_question() -> String {
    B_BY_MATCH.replace(
        "let d = match digit(s, i) {\n            Result.Ok(value: d) => d,\n            \
         Result.Err(error: e) => if true { return Result[Int, ParseError].Err(error: e); } \
         else { return Result[Int, ParseError].Err(error: e); },\n        };",
        "let d = digit(s, i)?;",
    )
}

#[test]
fn writing_a_match_as_a_question_mark_moves_no_interface() {
    // §59 and §103. B's body changes from an explicit `match` to `?`: B is checked again
    // and recompiled, its interface hash does not move, and so A is neither.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B_BY_MATCH);
    let before = interface_of(dir.path(), "b.nz");
    let before_hash =
        InterfaceHash::of(&PersistentUnitInterface::from_json(&before).unwrap()).unwrap();
    assert_eq!(check(dir.path(), "a.nz").0, (true, 3, 0));
    let cold = build(dir.path(), "a.nz");
    assert!(cold.0 > 0);

    let edited = b_by_question();
    assert_ne!(edited, B_BY_MATCH, "the body was not rewritten");
    write(dir.path(), "b.nz", &edited);
    let after = interface_of(dir.path(), "b.nz");
    assert_eq!(before, after, "a body edit moved B's interface");
    assert_eq!(
        before_hash,
        InterfaceHash::of(&PersistentUnitInterface::from_json(&after).unwrap()).unwrap()
    );

    // B alone is checked; A and the prelude are reused.
    assert_eq!(check(dir.path(), "a.nz").0, (true, 1, 2));
    // B's object misses — its IR changed — and A's is reused.
    let (compiled, reused, _, _) = build(dir.path(), "a.nz");
    assert_eq!(compiled, 1, "only B's unit should be recompiled");
    assert!(reused >= 1, "A's object should be reused");
    let native = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&native.stdout).trim(), "420");
}

#[test]
fn changing_an_exported_error_type_reaches_the_importer_and_its_question_marks() {
    // §104. `parse` now fails with `BadInput`: B's interface moves, A is checked again,
    // and A's `?` inside a `Result[Int, ParseError]` function is refused.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    let before = interface_of(dir.path(), "b.nz");
    assert!(check(dir.path(), "a.nz").0 .0);

    let changed = format!(
        "pub struct BadInput {{\n    at: Int,\n}}\n{}",
        B.replace(
            "pub fn parse(s: Str) -> Result[Int, ParseError]",
            "pub fn parse(s: Str) -> Result[Int, BadInput]"
        )
        .replace(
            "return Result[Int, ParseError].Err(error: ParseError(at: i));",
            "return Result[Int, BadInput].Err(error: BadInput(at: i));"
        )
        .replace(
            "Result[Int, ParseError].Ok(value: n)",
            "Result[Int, BadInput].Ok(value: n)"
        )
    );
    write(dir.path(), "b.nz", &changed);
    assert_ne!(before, interface_of(dir.path(), "b.nz"));
    let ((ok, checked, reused), codes) = check(dir.path(), "a.nz");
    assert!(!ok);
    assert_eq!(
        (checked, reused),
        (2, 1),
        "A and B checked, the prelude reused"
    );
    assert_eq!(codes, ["N0362", "N0362"], "both `?` sites");
}

/// The prelude's published interface and its hash, for a prelude with this source.
///
/// A **fixture**: the real prelude is never edited for a test. The source is put where
/// the prelude goes — the last file, under the prelude's key — which is all that makes a
/// module the prelude.
fn fixture_hash(prelude: &str) -> InterfaceHash {
    let map = SourceMap::from_parts(vec![
        ("m.nz".to_owned(), "fn main() -> Int { 0 }\n".to_owned()),
        ("@core/prelude.nz".to_owned(), prelude.to_owned()),
    ]);
    let mut graph = ModuleGraph::single();
    graph.attach_prelude(FileId::from_index(1));
    struct Capture(Option<InterfaceHash>);
    impl nazm_core::Reuse for Capture {
        fn plan(&mut self, graph: &ModuleGraph, interfaces: &[UnitInterface], types: &UserTypes) {
            let core = graph.prelude().expect("a prelude");
            let published = interfaces
                .get(core.index())
                .and_then(|i| PersistentUnitInterface::published(i, graph, types).ok())
                .expect("published");
            self.0 = InterfaceHash::of(&published).ok();
        }
        fn must_check(&self, _: ModuleId) -> bool {
            true
        }
        fn checked_clean(&mut self, _: ModuleId) {}
    }
    let (program, errors) = nazm_core::__syntax::parse_map(&map);
    assert!(errors.is_empty(), "{errors:#?}");
    let mut capture = Capture(None);
    let (_, errors) = nazm_core::check_reusing(&program, &graph, &mut capture, &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    capture.0.expect("a hash")
}

#[test]
fn a_public_change_to_the_prelude_moves_its_hash_and_a_private_one_does_not() {
    // §60 and §105, on fixtures. Every module's check key includes the prelude's interface
    // hash, as it includes any import's, so this is exactly what decides whether a change
    // to the prelude invalidates the modules that use it.
    let real = nazm_core::sema::prelude::SOURCE;
    let base = fixture_hash(real);

    let private_helper = format!("{real}\nfn helper() -> Int {{ 1 }}\n");
    assert_eq!(
        fixture_hash(&private_helper),
        base,
        "a private addition to the prelude moved its interface"
    );
    let commented = format!("// a note\n{real}");
    assert_eq!(fixture_hash(&commented), base);

    for (what, changed) in [
        (
            "a payload field is renamed",
            real.replace("Err(error: E)", "Err(reason: E)"),
        ),
        (
            "a variant is added",
            real.replace("    None,\n", "    None,\n    Unknown,\n"),
        ),
        (
            "a type parameter is added",
            real.replace("pub enum Option[T]", "pub enum Option[T, U]"),
        ),
        (
            "a public function is added",
            format!("{real}\npub fn helper() -> Int {{ 1 }}\n"),
        ),
    ] {
        assert_ne!(changed, real, "{what}: the fixture did not change");
        assert_ne!(
            fixture_hash(&changed),
            base,
            "{what} did not move the prelude's hash"
        );
    }
}

#[test]
fn the_prelude_has_no_native_unit_of_its_own() {
    // §100. It declares two enums and no function, so there is nothing to compile: its
    // enums' layouts and helpers are emitted in the units that use them. Two project
    // modules, the runtime and the entry are the whole build.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    let out = nazm(
        dir.path(),
        &["build", "a.nz", "-o", "prog", "--build-report", "--emit-ir"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = stdout
        .lines()
        .find(|l| l.contains("nazm.build-report/1"))
        .map(|l| serde_json::from_str(l).expect("a report"))
        .expect("a build report");
    assert_eq!(v["native"]["units"].as_u64(), Some(4), "{v}");
    for entry in std::fs::read_dir(dir.path()).expect("read dir").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "ll") {
            let ir = std::fs::read_to_string(&path).expect("read");
            assert!(
                !ir.contains("source_filename = \"@core/prelude\""),
                "{} is a unit for the prelude",
                path.display()
            );
        }
    }
}

#[test]
fn a_warm_build_of_a_result_heavy_program_compiles_nothing() {
    // §102: modules and generic instances alike, including instances whose bodies use `?`.
    let dir = tempfile::tempdir().expect("tempdir");
    a_and_b(dir.path(), B);
    write(
        dir.path(),
        "a.nz",
        &format!(
            "{A}\nfn pass[T, E](r: Result[T, E]) -> Result[T, E] {{\n    let v = r?;\n    Result[T, E].Ok(value: v)\n}}\n\
             fn more() -> Int {{\n    match pass(parse(\"12\")) {{ Result.Ok(value: n) => n, Result.Err(error: e) => 0, }}\n        \
             + match pass(Result[Str, Int].Err(error: 3)) {{ Result.Ok(value: s) => 0, Result.Err(error: n) => n, }}\n}}\n"
        ),
    );
    let (compiled, _, instances, _) = build(dir.path(), "a.nz");
    assert!(compiled >= 2 && instances >= 2, "{compiled} {instances}");
    let warm = build(dir.path(), "a.nz");
    assert_eq!(
        (warm.0, warm.2),
        (0, 0),
        "a warm build ran the object compiler"
    );
}
