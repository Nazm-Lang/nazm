//! Identity and interfaces, across processes and across checkouts.
//!
//! # Why these need the binary
//!
//! Everything N3 claims is about **two runs**. A test that builds an interface and reads
//! it back in the same process proves the types agree with themselves; it does not prove
//! that a key survives a process, that bytes do not depend on where a checkout sits, or
//! that a module can be checked with its dependency's source deleted. Each of those needs
//! a real boundary, so each of these runs `nazm` as a program.
//!
//! Nothing here is a cache. `nazm interface` writes what it is told and reads nothing
//! back; the invalidation table below is a statement about **fingerprints**, and the thing
//! that would act on it does not exist.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

use nazm_core::sema::key::ModuleKey;
use nazm_core::sema::module::ModuleGraph;
use nazm_core::sema::resolve::{FnId, Resolution};
use nazm_iface::{InterfaceHash, PersistentUnitInterface};
use nazm_span::FileId;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// The published interface of the module at `root`, as the binary writes it.
fn interface_of(root: &Path) -> String {
    let out = Command::new(NAZM)
        .args(["interface", root.to_str().unwrap()])
        .output()
        .expect("run nazm interface");
    assert!(
        out.status.success(),
        "nazm interface refused {}: {}",
        root.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Its fingerprint, as the binary reports it.
fn hash_of(root: &Path) -> String {
    let out = Command::new(NAZM)
        .args(["interface", root.to_str().unwrap(), "--hash"])
        .output()
        .expect("run nazm interface --hash");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A project in a fresh directory: `root.nz` importing `lib/util.nz`.
fn project(dir: &Path, util_body: &str, util_exports: &str) {
    write(
        dir,
        "root.nz",
        "use \"lib/util.nz\";\n\npub fn parse(s: Str) -> Int { twice(str_len(s)) }\n\n\
         fn secret() -> Int { 1 }\n\nfn main() -> Int { parse(\"abc\") + secret() }\n",
    );
    write(
        dir,
        "lib/util.nz",
        &format!("{util_exports}\n\nfn hidden() -> Int {{ {util_body} }}\n"),
    );
}

#[test]
fn the_same_project_gives_the_same_interface_in_two_processes() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), "5", "pub fn twice(n: Int) -> Int { n * 2 }");
    let root = dir.path().join("root.nz");

    // Two separate runs of the compiler. Every session-local id inside each is an index
    // into that run; nothing about them is expected to agree, and the bytes are.
    let one = interface_of(&root);
    let two = interface_of(&root);
    assert_eq!(one, two);
    assert_eq!(hash_of(&root), hash_of(&root));

    // And the key is the module's place in the project, not its place on this machine.
    assert!(one.contains(r#""module":"root.nz""#), "{one}");
    assert!(one.contains(r#""lib/util.nz""#), "{one}");
    assert!(
        !one.contains(dir.path().to_str().unwrap()),
        "an absolute path reached the interface: {one}"
    );
}

#[test]
fn two_checkouts_in_different_directories_give_identical_bytes() {
    // The test that catches an absolute path in a durable identity. Two directories with
    // nothing in common but their contents.
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    assert_ne!(a.path(), b.path());
    project(a.path(), "5", "pub fn twice(n: Int) -> Int { n * 2 }");
    project(b.path(), "5", "pub fn twice(n: Int) -> Int { n * 2 }");

    assert_eq!(
        interface_of(&a.path().join("root.nz")),
        interface_of(&b.path().join("root.nz"))
    );
    assert_eq!(
        hash_of(&a.path().join("root.nz")),
        hash_of(&b.path().join("root.nz"))
    );
}

#[test]
fn a_private_body_change_does_not_move_the_interface() {
    // The property the whole milestone is for: a module whose private helper changed
    // offers exactly what it offered, so its dependents are unaffected.
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    project(a.path(), "1", "pub fn twice(n: Int) -> Int { n * 2 }");
    project(b.path(), "999", "pub fn twice(n: Int) -> Int { n * 2 }");

    let one = a.path().join("lib/util.nz");
    let two = b.path().join("lib/util.nz");
    assert_eq!(interface_of(&one), interface_of(&two));
    assert_eq!(hash_of(&one), hash_of(&two));
}

#[test]
fn an_exported_body_change_does_not_move_the_interface_either() {
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    project(a.path(), "1", "pub fn twice(n: Int) -> Int { n * 2 }");
    project(b.path(), "1", "pub fn twice(n: Int) -> Int { n + n }");

    assert_eq!(
        hash_of(&a.path().join("lib/util.nz")),
        hash_of(&b.path().join("lib/util.nz"))
    );
}

#[test]
fn comments_and_layout_do_not_move_the_interface() {
    // It identifies what a module offers, not how the offer was typed. Nothing here is a
    // source hash, which is why this is true by construction rather than by a rule.
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    write(a.path(), "m.nz", "pub fn f(n: Int) -> Int { n }\n");
    write(
        b.path(),
        "m.nz",
        "// what it does\n///   with documentation\npub    fn f( n : Int )   ->   Int {\n    n\n}\n",
    );
    assert_eq!(
        interface_of(&a.path().join("m.nz")),
        interface_of(&b.path().join("m.nz"))
    );
}

/// One row of the invalidation table: does this edit move the fingerprint?
fn fingerprint_of_module(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "m.nz", source);
    hash_of(&dir.path().join("m.nz"))
}

#[test]
fn the_invalidation_table_is_what_the_fingerprint_actually_does() {
    // `docs/architecture.md` §7.3 states the model; this is the model measured. Each row
    // is an edit to one module, and the question is only ever whether that module's
    // **dependents** must be re-checked.
    let base = "pub fn shown(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 1 }\n";
    let unchanged: &[(&str, &str)] = &[
        (
            "a private body changes",
            "pub fn shown(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 999 }\n",
        ),
        (
            "a private signature changes",
            "pub fn shown(n: Int) -> Int { helper(1) + n }\nfn helper(k: Int) -> Int { k }\n",
        ),
        (
            "an exported body changes",
            "pub fn shown(n: Int) -> Int { helper() + n + 0 }\nfn helper() -> Int { 1 }\n",
        ),
        (
            "a private function is added",
            "pub fn shown(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 1 }\n\
             fn extra() -> Int { 2 }\n",
        ),
        (
            "a private function is removed",
            "pub fn shown(n: Int) -> Int { n }\n",
        ),
    ];
    let changed: &[(&str, &str)] = &[
        (
            "an exported signature changes",
            "pub fn shown(n: Str) -> Int { helper() + str_len(n) }\nfn helper() -> Int { 1 }\n",
        ),
        (
            "an exported return type changes",
            "pub fn shown(n: Int) -> Str { \"x\" }\nfn helper() -> Int { 1 }\n",
        ),
        (
            "a private function becomes exported",
            "pub fn shown(n: Int) -> Int { helper() + n }\npub fn helper() -> Int { 1 }\n",
        ),
        (
            "an exported function becomes private",
            "fn shown(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 1 }\n",
        ),
        (
            "an exported function is renamed",
            "pub fn displayed(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 1 }\n",
        ),
        (
            "an exported function is added",
            "pub fn shown(n: Int) -> Int { helper() + n }\npub fn also(n: Int) -> Int { n }\n\
             fn helper() -> Int { 1 }\n",
        ),
        (
            "an exported function is removed",
            "fn helper() -> Int { 1 }\npub fn other() -> Int { 2 }\n",
        ),
    ];

    let before = fingerprint_of_module(base);
    for (what, after) in unchanged {
        assert_eq!(
            before,
            fingerprint_of_module(after),
            "{what}: the fingerprint moved, so every dependent would be re-checked for nothing"
        );
    }
    for (what, after) in changed {
        assert_ne!(
            before,
            fingerprint_of_module(after),
            "{what}: the fingerprint did not move, so a dependent would keep a stale answer"
        );
    }
}

#[test]
fn a_dependent_checks_against_a_deserialised_interface_with_the_dependency_deleted() {
    // N3's strongest claim, and the only test that can establish it: B's interface crosses
    // a real serialization boundary — a separate process wrote it — and then B's source is
    // **deleted** before A is checked against it.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { n * 2 }\nfn hidden() -> Int { 7 }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { twice(21) }\n",
    );

    let published = interface_of(&dir.path().join("b.nz"));
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    assert!(!dir.path().join("b.nz").exists());

    // From here on, B exists only as bytes.
    let interface = PersistentUnitInterface::from_json(&published).expect("a valid interface");
    assert_eq!(interface.module_key(), ModuleKey::relative("b.nz"));

    let a_source = std::fs::read_to_string(dir.path().join("a.nz")).expect("read A");
    let (program, parse_errors) = nazm_core::__syntax::parse_source(&a_source);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(FileId::from_index(1), interface.module_key());
    graph.import(a, b, nazm_span::Span::synthetic());

    let units = [nazm_core::Unit::of(&program, &graph, a)];
    let unit = &units[0];
    let mut res = Resolution::default();
    let mut errors = Vec::new();
    // B's records are materialised into this session's table before anything that could
    // name one, which is the order a compilation uses too.
    let rehydrated = interface.rehydrate(b, FnId::from_index(4096), res.records_mut());
    let own = nazm_core::declare_records(&units, &mut res, &mut errors);
    let _ = own;
    nazm_core::declare_unit(unit, &[&rehydrated], &mut res, &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");

    nazm_core::check_unit(unit, &[&rehydrated], &mut res, &mut errors);
    assert!(
        errors.is_empty(),
        "A should check against B's persisted interface alone: {errors:#?}"
    );

    // And the negative half, so the pass above is not something else succeeding: with an
    // interface that offers nothing, the same source is refused.
    let empty = PersistentUnitInterface::from_json(
        r#"{"schema":"nazm.interface/2","module":"b.nz","dependencies":[],"records":[],"exports":[]}"#,
    )
    .expect("valid");
    let mut errors = Vec::new();
    let mut empty_res = Resolution::default();
    let rehydrated_empty = empty.rehydrate(b, FnId::from_index(4096), empty_res.records_mut());
    nazm_core::check_unit(unit, &[&rehydrated_empty], &mut empty_res, &mut errors);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].code, nazm_diag::codes::UNDEFINED_FUNCTION);

    // A private definition of B's was never in the interface, so it is not nameable from
    // it either — the isolation is a property of the bytes, not of a filter at use.
    assert!(!published.contains("hidden"), "{published}");
}

#[test]
fn a_module_outside_the_source_root_declines_a_durable_identity() {
    // Not an error: it checks, runs and compiles. It simply has no key, because a key for
    // it would depend on where the root happens to sit — which is the one thing a durable
    // identity may not depend on.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "outside.nz", "pub fn v() -> Int { 1 }\n");
    write(
        dir.path(),
        "app/main.nz",
        "use \"../outside.nz\";\nfn main() -> Int { v() }\n",
    );
    let root = dir.path().join("app/main.nz");

    let ran = Command::new(NAZM)
        .args(["run", root.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "1");

    let published = Command::new(NAZM)
        .args(["interface", root.to_str().unwrap()])
        .output()
        .expect("run nazm interface");
    assert!(!published.status.success());
    let err = String::from_utf8_lossy(&published.stderr);
    assert!(err.contains("durable identity"), "{err}");
}

#[test]
fn an_interface_names_only_its_direct_dependencies() {
    // Non-transitivity, in the persisted form: what A depends on is what A imported, and a
    // dependency's own dependencies are its business.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "c.nz", "pub fn deep() -> Int { 7 }\n");
    write(
        dir.path(),
        "b.nz",
        "use \"c.nz\";\npub fn mid() -> Int { deep() }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\npub fn top() -> Int { mid() }\n",
    );

    let published = interface_of(&dir.path().join("a.nz"));
    assert!(
        published.contains(r#""dependencies":["b.nz"]"#),
        "{published}"
    );
    assert!(!published.contains("c.nz"), "{published}");
}

#[test]
fn the_entry_point_is_not_part_of_what_a_module_offers() {
    // `main` is private by default and a program's entry point either way; nothing about
    // it belongs in an interface unless it was exported like anything else.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "pub fn v() -> Int { 1 }\nfn main() -> Int { v() }\n",
    );
    let published = interface_of(&dir.path().join("m.nz"));
    assert!(published.contains("\"v\""), "{published}");
    assert!(!published.contains("main"), "{published}");
}

#[test]
fn a_fingerprint_is_the_algorithm_and_the_digest() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "m.nz", "pub fn v() -> Int { 1 }\n");
    let line = hash_of(&dir.path().join("m.nz"));
    let (algorithm, digest) = line.split_once(' ').expect("algorithm and digest");
    assert_eq!(algorithm, InterfaceHash::ALGORITHM);
    assert!(InterfaceHash::parse(digest).is_some(), "{line}");

    // And it is the fingerprint of the bytes the other subcommand printed, rather than of
    // something computed a second way.
    let published = interface_of(&dir.path().join("m.nz"));
    let parsed = PersistentUnitInterface::from_json(&published).expect("valid");
    assert_eq!(InterfaceHash::of(&parsed).unwrap().to_string(), digest);
}

#[test]
fn session_ids_differ_between_runs_where_durable_keys_do_not() {
    // The point of keeping both layers. The same module is `ModuleId(1)` in one
    // compilation and `ModuleId(2)` in another — load order decides — while its
    // `ModuleKey` is `lib/util.nz` in both.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "lib/util.nz", "pub fn v() -> Int { 1 }\n");
    write(dir.path(), "other.nz", "pub fn w() -> Int { 2 }\n");
    write(
        dir.path(),
        "one.nz",
        "use \"lib/util.nz\";\nuse \"other.nz\";\nfn main() -> Int { v() + w() }\n",
    );
    write(
        dir.path(),
        "two.nz",
        "use \"other.nz\";\nuse \"lib/util.nz\";\nfn main() -> Int { v() + w() }\n",
    );

    // `lib/util.nz` is module 1 in one compilation and module 2 in the other, and its
    // interface is the same bytes either way — it does not depend on the compilation that
    // published it.
    for root in ["one.nz", "two.nz"] {
        let out = Command::new(NAZM)
            .args(["check", dir.path().join(root).to_str().unwrap()])
            .output()
            .expect("check");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(
        interface_of(&dir.path().join("lib/util.nz")),
        interface_of(&dir.path().join("lib/util.nz"))
    );
}

/// Kept last because it is the one row of the table that is about a module's *own* inputs
/// rather than its dependents'.
#[test]
fn changing_which_modules_are_imported_changes_the_interface() {
    // A module's direct dependencies are part of what it publishes, so a change to them
    // moves its fingerprint — which is right: an importer of A that cached "A's interface"
    // has a value that no longer describes A. What it does **not** mean is that A's
    // importers must re-check for an unrelated reason; A itself re-checks, and if its
    // exports are unchanged its own importers are unaffected, which is the next hop of the
    // model in `architecture.md` §7.3.
    let a = tempfile::tempdir().expect("tempdir");
    write(a.path(), "dep.nz", "pub fn d() -> Int { 1 }\n");
    write(a.path(), "m.nz", "pub fn v() -> Int { 1 }\n");
    let without = hash_of(&a.path().join("m.nz"));

    write(
        a.path(),
        "m.nz",
        "use \"dep.nz\";\npub fn v() -> Int { d() }\n",
    );
    let with = hash_of(&a.path().join("m.nz"));
    assert_ne!(without, with);
}

// --- records in the persisted interface ------------------------------------------------

#[test]
fn an_exported_record_is_published_with_its_fields_and_durable_identity() {
    // The whole shape, asserted as bytes: a record is named by its `DefKey`, its fields
    // are sorted by name, and a field whose type is a record is named by that record's
    // identity rather than by the spelling it happens to have.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "pub struct Point {\n    y: Int,\n    x: Str,\n}\n\
         struct Hidden {\n    n: Int,\n}\n\
         pub fn at(p: Point) -> Point { p }\n\
         fn secret() -> Int { Hidden(n: 1).n }\n",
    );
    let json = interface_of(&dir.path().join("m.nz"));
    assert_eq!(
        json,
        "{\"schema\":\"nazm.interface/2\",\"module\":\"m.nz\",\"dependencies\":[],\
         \"records\":[{\"def\":\"m.nz::record Point\",\"name\":\"Point\",\
         \"fields\":[{\"name\":\"x\",\"ty\":\"Str\"},{\"name\":\"y\",\"ty\":\"Int\"}]}],\
         \"exports\":[{\"def\":\"m.nz::fn at\",\"name\":\"at\",\"kind\":\"fn\",\
         \"params\":[\"m.nz::record Point\"],\"ret\":\"m.nz::record Point\"}]}"
    );
    // A private record is not offered, and its name is not in the bytes.
    assert!(!json.contains("Hidden"), "{json}");
}

#[test]
fn moving_two_field_declarations_does_not_move_the_interface() {
    // `docs/spec.md` says field order is not semantic. This is that claim measured: the
    // persisted form sorts by name, so the bytes — and therefore the fingerprint — cannot
    // move for an edit the language calls meaningless.
    let one = "pub struct Point {\n    x: Int,\n    y: Str,\n}\n";
    let other = "pub struct Point {\n    y: Str,\n    x: Int,\n}\n";
    assert_eq!(fingerprint_of_module(one), fingerprint_of_module(other));
}

#[test]
fn the_record_invalidation_table_is_what_the_fingerprint_actually_does() {
    // The same shape as the function table above, for records. Each row is an edit to a
    // module, and the question is only ever whether its **dependents** must re-check.
    let base = "pub struct Point {\n    x: Int,\n    y: Int,\n}\n\
                struct Hidden {\n    n: Int,\n}\n\
                pub fn at() -> Point { Point(x: 1, y: 2) }\n";
    let unchanged: &[(&str, &str)] = &[
        (
            "a private record's fields change",
            "pub struct Point {\n    x: Int,\n    y: Int,\n}\n\
             struct Hidden {\n    n: Str,\n    extra: Int,\n}\n\
             pub fn at() -> Point { Point(x: 1, y: 2) }\n",
        ),
        (
            "a private record is added",
            "pub struct Point {\n    x: Int,\n    y: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             struct Another {\n    m: Int,\n}\n\
             pub fn at() -> Point { Point(x: 1, y: 2) }\n",
        ),
        (
            "an exported record's fields are reordered",
            "pub struct Point {\n    y: Int,\n    x: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             pub fn at() -> Point { Point(x: 1, y: 2) }\n",
        ),
    ];
    let changed: &[(&str, &str)] = &[
        (
            "an exported field is added",
            "pub struct Point {\n    x: Int,\n    y: Int,\n    z: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             pub fn at() -> Point { Point(x: 1, y: 2, z: 3) }\n",
        ),
        (
            "an exported field is removed",
            "pub struct Point {\n    x: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             pub fn at() -> Point { Point(x: 1) }\n",
        ),
        (
            "an exported field changes type",
            "pub struct Point {\n    x: Str,\n    y: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             pub fn at() -> Point { Point(x: \"a\", y: 2) }\n",
        ),
        (
            "an exported record is renamed",
            "pub struct Place {\n    x: Int,\n    y: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             pub fn at() -> Place { Place(x: 1, y: 2) }\n",
        ),
        (
            "an exported record stops being exported",
            "struct Point {\n    x: Int,\n    y: Int,\n}\n\
             struct Hidden {\n    n: Int,\n}\n\
             fn at() -> Point { Point(x: 1, y: 2) }\n",
        ),
    ];

    let before = fingerprint_of_module(base);
    for (what, after) in unchanged {
        assert_eq!(
            before,
            fingerprint_of_module(after),
            "the fingerprint moved when {what}, and it should not have"
        );
    }
    for (what, after) in changed {
        assert_ne!(
            before,
            fingerprint_of_module(after),
            "the fingerprint did not move when {what}, and it should have"
        );
    }
}

#[test]
fn an_exported_record_reaches_a_third_modules_shape_through_the_interface() {
    // Closure over field types. `b.nz` exports a record whose field is a record declared
    // in `c.nz`; an importer of `b.nz` alone has to understand that shape to project
    // through it, and cannot go and read a module it never imported. So the definition
    // travels — under **its own** module's identity, which is what says it is not `b`'s.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "c.nz", "pub struct Inner {\n    n: Int,\n}\n");
    write(
        dir.path(),
        "b.nz",
        "use \"c.nz\";\npub struct Wrapper {\n    inner: Inner,\n}\n",
    );
    let json = interface_of(&dir.path().join("b.nz"));
    assert!(json.contains("c.nz::record Inner"), "{json}");
    assert!(json.contains("b.nz::record Wrapper"), "{json}");
    // Carrying a shape is not carrying a name: nothing here makes `Inner` writable by an
    // importer of `b.nz`, which is what the non-transitive import rule is about.
    refused_import(&dir);
}

/// A module importing `b.nz` may not write `Inner`, whose shape its interface carries.
fn refused_import(dir: &tempfile::TempDir) {
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { Inner(n: 1).n }\n",
    );
    let out = Command::new(NAZM)
        .args(["check", dir.path().join("a.nz").to_str().unwrap()])
        .output()
        .expect("check");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("N0305"), "{text}");
}

#[test]
fn a_dependent_checks_records_against_a_deserialised_interface_with_the_dependency_deleted() {
    // N2's strongest boundary, asked of records: construction, projection and a
    // record-valued signature, checked from bytes alone with the dependency's source not
    // on disk. Then the same source against an interface whose field was removed, which
    // must be refused — otherwise the first half proves only that nothing was checked.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "b.nz",
        "pub struct Point {\n    x: Int,\n    label: Str,\n}\n\
         pub fn at(n: Int) -> Point { Point(x: n, label: \"p\") }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         fn size(p: Point) -> Int { p.x + str_len(p.label) }\n\
         fn main() -> Int { size(at(3)) + Point(x: 1, label: \"\").x }\n",
    );

    let published = interface_of(&dir.path().join("b.nz"));
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    assert!(!dir.path().join("b.nz").exists());

    let interface = PersistentUnitInterface::from_json(&published).expect("a valid interface");
    let a_source = std::fs::read_to_string(dir.path().join("a.nz")).expect("read A");
    let (program, parse_errors) = nazm_core::__syntax::parse_source(&a_source);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(FileId::from_index(1), interface.module_key());
    graph.import(a, b, nazm_span::Span::synthetic());

    let check_against = |json: &str| -> Vec<nazm_diag::Diagnostic> {
        let iface = PersistentUnitInterface::from_json(json).expect("a valid interface");
        let units = [nazm_core::Unit::of(&program, &graph, a)];
        let mut res = Resolution::default();
        let mut errors = Vec::new();
        let rehydrated = iface.rehydrate(b, FnId::from_index(4096), res.records_mut());
        nazm_core::declare_records(&units, &mut res, &mut errors);
        nazm_core::declare_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        nazm_core::check_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        errors
    };

    assert!(
        check_against(&published).is_empty(),
        "A should check against B's persisted interface alone: {:#?}",
        check_against(&published)
    );

    // The negative half: the same source, and an interface whose record lost the field
    // `A` projects.
    let narrowed = published.replace(",{\"name\":\"x\",\"ty\":\"Int\"}", "");
    let narrowed = if narrowed == published {
        published.replace("{\"name\":\"label\",\"ty\":\"Str\"},", "")
    } else {
        narrowed
    };
    assert_ne!(narrowed, published, "the interface was not narrowed");
    let errors = check_against(&narrowed);
    assert!(
        errors
            .iter()
            .any(|d| d.code == nazm_diag::codes::UNKNOWN_FIELD
                || d.code == nazm_diag::codes::MISSING_FIELD),
        "a removed field should be refused: {errors:#?}"
    );
}
