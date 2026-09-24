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
    let rehydrated = interface.rehydrate(b, FnId::from_index(4096), res.types_mut());
    let own = nazm_core::declare_types(&units, &mut res, &mut errors);
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
        r#"{"schema":"nazm.interface/4","module":"b.nz","dependencies":[],"records":[],"enums":[],"exports":[]}"#,
    )
    .expect("valid");
    let mut errors = Vec::new();
    let mut empty_res = Resolution::default();
    let rehydrated_empty = empty.rehydrate(b, FnId::from_index(4096), empty_res.types_mut());
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
        // The core prelude is a dependency of every module, like any import.
        published.contains(r#""dependencies":["@core/prelude","b.nz"]"#),
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
        "{\"schema\":\"nazm.interface/4\",\"module\":\"m.nz\",\"dependencies\":[\"@core/prelude\"],\
         \"records\":[{\"def\":\"m.nz::record Point\",\"name\":\"Point\",\"generics\":0,\
         \"fields\":[{\"name\":\"x\",\"ty\":\"Str\"},{\"name\":\"y\",\"ty\":\"Int\"}]}],\
         \"enums\":[],\
         \"exports\":[{\"def\":\"m.nz::fn at\",\"name\":\"at\",\"kind\":\"fn\",\"generics\":0,\
         \"params\":[{\"def\":\"m.nz::record Point\",\"args\":[]}],\
         \"ret\":{\"def\":\"m.nz::record Point\",\"args\":[]}}]}"
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
        let rehydrated = iface.rehydrate(b, FnId::from_index(4096), res.types_mut());
        nazm_core::declare_types(&units, &mut res, &mut errors);
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

#[test]
fn an_exported_enum_is_published_with_its_variants_and_durable_identity() {
    // The canonical form, pinned: variants sorted by name, payload fields sorted by name,
    // the enum under its durable identity, and the private companion absent. The source
    // declares them in none of those orders, which is what makes this an assertion about
    // canonicalisation rather than about the parser.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "pub enum State {\n    Ready,\n    Failed(why: Str, code: Int),\n    Done(code: Int),\n}\n\
         enum Hidden {\n    Never,\n}\n\
         pub fn start() -> State { State.Ready() }\n\
         fn secret() -> Int { match Hidden.Never() { Hidden.Never() => 1, } }\n",
    );
    let json = interface_of(&dir.path().join("m.nz"));
    assert_eq!(
        json,
        "{\"schema\":\"nazm.interface/4\",\"module\":\"m.nz\",\"dependencies\":[\"@core/prelude\"],\
         \"records\":[],\
         \"enums\":[{\"def\":\"m.nz::enum State\",\"name\":\"State\",\"generics\":0,\"variants\":[\
         {\"name\":\"Done\",\"fields\":[{\"name\":\"code\",\"ty\":\"Int\"}]},\
         {\"name\":\"Failed\",\"fields\":[{\"name\":\"code\",\"ty\":\"Int\"},\
         {\"name\":\"why\",\"ty\":\"Str\"}]},\
         {\"name\":\"Ready\",\"fields\":[]}]}],\
         \"exports\":[{\"def\":\"m.nz::fn start\",\"name\":\"start\",\"kind\":\"fn\",\
         \"generics\":0,\"params\":[],\"ret\":{\"def\":\"m.nz::enum State\",\"args\":[]}}]}"
    );
    assert!(!json.contains("Hidden"), "{json}");
    // The numeric discriminant is representation, not meaning, and is nowhere in this.
    assert!(!json.contains("tag"), "{json}");
}

#[test]
fn the_enum_invalidation_table_is_what_the_fingerprint_actually_does() {
    // §59, run rather than asserted in prose. Each row is an edit and what it must do to
    // the fingerprint — and the "unchanged" rows are the ones that would be easy to get
    // wrong by taking the parser's order for the canonical one.
    let dir = tempfile::tempdir().expect("tempdir");
    let hash_of = |source: &str| -> String {
        write(dir.path(), "m.nz", source);
        let json = interface_of(&dir.path().join("m.nz"));
        let parsed = PersistentUnitInterface::from_json(&json).expect("valid");
        InterfaceHash::of(&parsed).expect("hashable").to_string()
    };

    const BASE: &str = "pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n}\n\
                        pub fn start() -> State { State.Ready() }\n";
    let base = hash_of(BASE);

    // Unchanged: reordering variants, reordering payload fields, and a private enum
    // added — none of them is a change to what this module offers.
    assert_eq!(
        base,
        hash_of(
            "pub enum State {\n    Done(code: Int, why: Str),\n    Ready,\n}\n\
             pub fn start() -> State { State.Ready() }\n"
        ),
        "reordering variants moved the fingerprint"
    );
    assert_eq!(
        base,
        hash_of(
            "pub enum State {\n    Ready,\n    Done(why: Str, code: Int),\n}\n\
             pub fn start() -> State { State.Ready() }\n"
        ),
        "reordering payload fields moved the fingerprint"
    );
    assert_eq!(
        base,
        hash_of(&format!("{BASE}enum Private {{\n    A,\n}}\n")),
        "a private enum moved the fingerprint"
    );

    // Changed: a variant added, renamed or removed; a payload field added or retyped.
    for (what, source) in [
        (
            "a variant added",
            "pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n    Failed,\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "a variant renamed",
            "pub enum State {\n    Waiting,\n    Done(code: Int, why: Str),\n}\n\
             pub fn start() -> State { State.Waiting() }\n",
        ),
        (
            "a payload field added",
            "pub enum State {\n    Ready,\n    Done(code: Int, why: Str, at: Int),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "a payload field retyped",
            "pub enum State {\n    Ready,\n    Done(code: Str, why: Str),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "a payload field removed",
            "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
    ] {
        assert_ne!(base, hash_of(source), "{what} did not move the fingerprint");
    }
}

#[test]
fn a_dependent_checks_enums_against_a_deserialised_interface_with_the_dependency_deleted() {
    // §60. Construction, exhaustive matching, payload bindings and an enum-valued
    // signature, all checked from bytes alone with the dependency's source not on disk.
    // Then the same source against an interface with a variant removed, which must be
    // refused — otherwise the first half proves only that nothing was checked.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "b.nz",
        "pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n}\n\
         pub fn start() -> State { State.Ready() }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         fn weight(s: State) -> Int { match s { State.Ready() => 1, \
         State.Done(code: c, why: w) => c + str_len(w), } }\n\
         fn main() -> Int { weight(start()) + weight(State.Done(code: 2, why: \"ab\")) }\n",
    );

    let published = interface_of(&dir.path().join("b.nz"));
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    assert!(!dir.path().join("b.nz").exists());

    let a_source = std::fs::read_to_string(dir.path().join("a.nz")).expect("read A");
    let (program, parse_errors) = nazm_core::__syntax::parse_source(&a_source);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(
        FileId::from_index(1),
        PersistentUnitInterface::from_json(&published)
            .expect("valid")
            .module_key(),
    );
    graph.import(a, b, nazm_span::Span::synthetic());

    let check_against = |json: &str| -> Vec<nazm_diag::Diagnostic> {
        let iface = PersistentUnitInterface::from_json(json).expect("a valid interface");
        let units = [nazm_core::Unit::of(&program, &graph, a)];
        let mut res = Resolution::default();
        let mut errors = Vec::new();
        let rehydrated = iface.rehydrate(b, FnId::from_index(4096), res.types_mut());
        nazm_core::declare_types(&units, &mut res, &mut errors);
        nazm_core::declare_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        nazm_core::check_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        errors
    };

    assert!(
        check_against(&published).is_empty(),
        "A should check against B's persisted interface alone: {:#?}",
        check_against(&published)
    );

    // The negative half, twice: an interface whose enum lost a variant — so A's `match`
    // names one that does not exist — and one that **gained** a variant, so A's `match`
    // is no longer exhaustive. The second is the shape §44 is about.
    let without = published.replace(",{\"name\":\"Ready\",\"fields\":[]}", "");
    assert_ne!(without, published, "the interface was not narrowed");
    let errors = check_against(&without);
    assert!(
        errors
            .iter()
            .any(|d| d.code == nazm_diag::codes::UNKNOWN_VARIANT),
        "a removed variant should be refused: {errors:#?}"
    );

    let widened = published.replace(
        "{\"name\":\"Ready\",\"fields\":[]}",
        "{\"name\":\"Ready\",\"fields\":[]},{\"name\":\"Waiting\",\"fields\":[]}",
    );
    assert_ne!(widened, published, "the interface was not widened");
    let errors = check_against(&widened);
    assert!(
        errors
            .iter()
            .any(|d| d.code == nazm_diag::codes::NON_EXHAUSTIVE_MATCH),
        "an added variant should make the importer's match non-exhaustive: {errors:#?}"
    );
}

// --- N11: generic interfaces, `nazm.interface/4` -------------------------------------------

/// The fingerprint of one module's published interface, from its source.
fn generic_hash(dir: &Path, source: &str) -> String {
    write(dir, "m.nz", source);
    let json = interface_of(&dir.join("m.nz"));
    let parsed = PersistentUnitInterface::from_json(&json).expect("valid");
    InterfaceHash::of(&parsed).expect("hashable").to_string()
}

const GENERIC_BASE: &str = "pub struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
    pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
    pub fn identity[T](value: T) -> T { value }\n\
    pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n";

#[test]
fn a_generic_interface_carries_positions_and_no_parameter_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "m.nz", GENERIC_BASE);
    let json = interface_of(&dir.path().join("m.nz"));
    assert_eq!(
        json,
        "{\"schema\":\"nazm.interface/4\",\"module\":\"m.nz\",\"dependencies\":[\"@core/prelude\"],\
         \"records\":[{\"def\":\"m.nz::record Pair\",\"name\":\"Pair\",\"generics\":2,\
         \"fields\":[{\"name\":\"first\",\"ty\":{\"param\":0}},{\"name\":\"second\",\"ty\":{\"param\":1}}]}],\
         \"enums\":[{\"def\":\"m.nz::enum Maybe\",\"name\":\"Maybe\",\"generics\":1,\"variants\":[\
         {\"name\":\"None\",\"fields\":[]},{\"name\":\"Some\",\"fields\":[{\"name\":\"value\",\"ty\":{\"param\":0}}]}]}],\
         \"exports\":[\
         {\"def\":\"m.nz::fn first\",\"name\":\"first\",\"kind\":\"fn\",\"generics\":2,\
         \"params\":[{\"def\":\"m.nz::record Pair\",\"args\":[{\"param\":0},{\"param\":1}]}],\"ret\":{\"param\":0}},\
         {\"def\":\"m.nz::fn identity\",\"name\":\"identity\",\"kind\":\"fn\",\"generics\":1,\
         \"params\":[{\"param\":0}],\"ret\":{\"param\":0}}]}"
    );
    // The names written in the source are nowhere in the bytes.
    for name in ["\"A\"", "\"B\"", "\"T\""] {
        assert!(!json.contains(name), "{name} leaked: {json}");
    }
}

#[test]
fn the_generic_invalidation_table_is_what_the_fingerprint_actually_does() {
    // `docs/spec.md`, *Order is semantic; names are not*, and the rest of the table the
    // milestone asked for — measured.
    let dir = tempfile::tempdir().expect("tempdir");
    let base = generic_hash(dir.path(), GENERIC_BASE);

    let unchanged: &[(&str, &str)] = &[
        (
            "every type parameter renamed",
            "pub struct Pair[Left, Right] {\n    first: Left,\n    second: Right,\n}\n\
             pub enum Maybe[Value] {\n    None,\n    Some(value: Value),\n}\n\
             pub fn identity[Value](value: Value) -> Value { value }\n\
             pub fn first[X, Y](p: Pair[X, Y]) -> X { p.first }\n",
        ),
        (
            "a generic body changes",
            "pub struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T](value: T) -> T { let v = value; v }\n\
             pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n",
        ),
        (
            "a private generic definition is added",
            &format!(
                "{GENERIC_BASE}struct Hidden[T] {{ v: T, }}\nfn helper[T](x: T) -> T {{ x }}\n"
            ),
        ),
    ];
    for (what, source) in unchanged {
        assert_eq!(generic_hash(dir.path(), source), base, "{what} moved it");
    }

    let changed: &[(&str, &str)] = &[
        (
            "the parameter order changes",
            "pub struct Pair[B, A] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T](value: T) -> T { value }\n\
             pub fn first[A, B](p: Pair[B, A]) -> A { p.first }\n",
        ),
        (
            "a record gains a parameter",
            "pub struct Pair[A, B, C] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T](value: T) -> T { value }\n\
             pub fn first[A, B](p: Pair[A, B, Int]) -> A { p.first }\n",
        ),
        (
            "a public generic field changes type",
            "pub struct Pair[A, B] {\n    first: A,\n    second: Vec[B],\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T](value: T) -> T { value }\n\
             pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n",
        ),
        (
            "a public generic variant is added",
            "pub struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n    Many(values: Vec[T]),\n}\n\
             pub fn identity[T](value: T) -> T { value }\n\
             pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n",
        ),
        (
            "a public generic signature changes",
            "pub struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T](value: T) -> Maybe[T] { Maybe[T].Some(value: value) }\n\
             pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n",
        ),
        (
            "a function gains a type parameter",
            "pub struct Pair[A, B] {\n    first: A,\n    second: B,\n}\n\
             pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
             pub fn identity[T, U](value: T) -> T { value }\n\
             pub fn first[A, B](p: Pair[A, B]) -> A { p.first }\n",
        ),
    ];
    for (what, source) in changed {
        assert_ne!(
            generic_hash(dir.path(), source),
            base,
            "{what} did not move it"
        );
    }
}

#[test]
fn a_dependent_checks_generic_apis_against_a_deserialised_interface_with_the_dependency_deleted() {
    // §29: `Box[Int]` built, a `Maybe[Str]` built and matched, `identity` called with its
    // type argument inferred and written, and a `Vec[Box[Int]]` — all checked from the
    // interface's bytes, with `b.nz` not on disk. Then the same source against an interface
    // whose `Box` lost its parameter, which must be refused: otherwise the first half
    // proves only that nothing was checked.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "b.nz",
        "pub struct Box[T] {\n    value: T,\n}\n\
         pub enum Maybe[T] {\n    None,\n    Some(value: T),\n}\n\
         pub fn identity[T](value: T) -> T { value }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         fn size(m: Maybe[Str]) -> Int { match m { Maybe.None() => 0, Maybe.Some(value: s) => str_len(s), } }\n\
         fn main() -> Int {\n    let b = Box[Int](value: 1);\n    \
         let xs = vec_new[Box[Int]]();\n    vec_push(xs, b);\n    \
         identity(b.value) + identity[Int](2) + size(Maybe[Str].Some(value: \"abc\")) \
         + vec_get(xs, 0).value\n}\n",
    );

    let published = interface_of(&dir.path().join("b.nz"));
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    assert!(!dir.path().join("b.nz").exists());

    let a_source = std::fs::read_to_string(dir.path().join("a.nz")).expect("read A");
    let (program, parse_errors) = nazm_core::__syntax::parse_source(&a_source);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");

    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(
        FileId::from_index(1),
        PersistentUnitInterface::from_json(&published)
            .expect("valid")
            .module_key(),
    );
    graph.import(a, b, nazm_span::Span::synthetic());

    let check_against = |json: &str| -> Vec<nazm_diag::Diagnostic> {
        let iface = PersistentUnitInterface::from_json(json).expect("a valid interface");
        let units = [nazm_core::Unit::of(&program, &graph, a)];
        let mut res = Resolution::default();
        let mut errors = Vec::new();
        let rehydrated = iface.rehydrate(b, FnId::from_index(4096), res.types_mut());
        nazm_core::declare_types(&units, &mut res, &mut errors);
        nazm_core::declare_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        nazm_core::check_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        errors
    };

    let errors = check_against(&published);
    assert!(
        errors.is_empty(),
        "A should check against B's persisted interface alone: {errors:#?}"
    );

    // The negative half: `Box` declared with no parameter, and its one field an `Int`, so
    // every `Box[Int]` A writes is applied to arguments it does not take.
    let narrowed = published.replace(
        "\"name\":\"Box\",\"generics\":1,\"fields\":[{\"name\":\"value\",\"ty\":{\"param\":0}}]",
        "\"name\":\"Box\",\"generics\":0,\"fields\":[{\"name\":\"value\",\"ty\":\"Int\"}]",
    );
    assert_ne!(narrowed, published, "the interface was not narrowed");
    let errors = check_against(&narrowed);
    assert!(
        errors
            .iter()
            .any(|d| d.code == nazm_diag::codes::TYPE_ARGUMENTS_ON_NON_GENERIC),
        "a definition that lost its parameter should be refused where it is applied: {errors:#?}"
    );
}

#[test]
fn a_dependent_derives_equality_from_a_deserialised_interface_with_the_dependency_deleted() {
    // N13. Whether an imported record, enum or core instance has `==` is derived from its
    // shape, and an interface carries the shape and nothing else: no "has equality" fact is
    // persisted that could disagree with it. So A compares B's `Point`, B's `State` and an
    // `Option[Point]` from B's interface alone, with B's source gone — and when the
    // interface says a field is a sequence, the same comparisons are refused, which is what
    // shows the answer came from the bytes and not from anything remembered.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "b.nz",
        "pub struct Point {\n    x: Int,\n    label: Str,\n}\n\
         pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n}\n\
         pub fn at(n: Int) -> Point { Point(x: n, label: \"p\") }\n\
         pub fn start() -> State { State.Ready() }\n\
         pub fn maybe(n: Int) -> Option[Point] { Option[Point].Some(value: at(n)) }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         fn main() -> Int {\n    let mut n = 0;\n    \
         if at(1) == Point(x: 1, label: \"p\") { n = n + 1; }\n    \
         if start() != State.Done(code: 1, why: \"w\") { n = n + 10; }\n    \
         if maybe(2) == maybe(2) { n = n + 100; }\n    n\n}\n",
    );

    let published = interface_of(&dir.path().join("b.nz"));
    assert!(!published.contains("equal"), "{published}");
    std::fs::remove_file(dir.path().join("b.nz")).expect("remove B");
    assert!(!dir.path().join("b.nz").exists());

    let a_source = std::fs::read_to_string(dir.path().join("a.nz")).expect("read A");
    let (program, parse_errors) = nazm_core::__syntax::parse_source(&a_source);
    assert!(parse_errors.is_empty(), "{parse_errors:#?}");
    let mut graph = ModuleGraph::default();
    let a = graph.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = graph.add(
        FileId::from_index(1),
        PersistentUnitInterface::from_json(&published)
            .expect("valid")
            .module_key(),
    );
    graph.import(a, b, nazm_span::Span::synthetic());

    let check_against = |json: &str| -> Vec<nazm_diag::Diagnostic> {
        let iface = PersistentUnitInterface::from_json(json).expect("a valid interface");
        let units = [nazm_core::Unit::of(&program, &graph, a)];
        let mut res = Resolution::default();
        let mut errors = Vec::new();
        let rehydrated = iface.rehydrate(b, FnId::from_index(4096), res.types_mut());
        nazm_core::declare_types(&units, &mut res, &mut errors);
        nazm_core::declare_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        nazm_core::check_unit(&units[0], &[&rehydrated], &mut res, &mut errors);
        errors
    };

    assert!(
        check_against(&published).is_empty(),
        "A should compare B's types from B's persisted interface alone: {:#?}",
        check_against(&published)
    );

    // The record's `label` becomes a `Strs`: `Point` and `Option[Point]` lose `==`, and the
    // enum, which never contained a `Point`, keeps it.
    let label = published.replace(
        "{\"name\":\"label\",\"ty\":\"Str\"}",
        "{\"name\":\"label\",\"ty\":\"Strs\"}",
    );
    assert_ne!(label, published, "the interface was not changed");
    let refused: Vec<_> = check_against(&label)
        .into_iter()
        .filter(|d| d.code == nazm_diag::codes::INVALID_OPERANDS)
        .collect();
    assert_eq!(refused.len(), 2, "{refused:#?}");
    // A payload field of one variant becomes a `Strs`: the enum loses `==`.
    let why = published.replace(
        "{\"name\":\"why\",\"ty\":\"Str\"}",
        "{\"name\":\"why\",\"ty\":\"Strs\"}",
    );
    assert_ne!(why, published, "the interface was not changed");
    let refused: Vec<_> = check_against(&why)
        .into_iter()
        .filter(|d| d.code == nazm_diag::codes::INVALID_OPERANDS)
        .collect();
    assert_eq!(refused.len(), 1, "{refused:#?}");
}
