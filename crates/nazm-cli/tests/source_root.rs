//! Where a project begins, and what that decides.
//!
//! # The property being established
//!
//! N3 derived the source root from the root module's directory, so `lib/common.nz` was
//! `lib/common.nz` in one compilation and `common.nz` in another — correct in each, and
//! useless to anything comparing the two. These tests are the closing of that: under one
//! declared root, a module has one identity whichever file the compilation started from,
//! whichever directory the checkout sits in, and whichever directory the shell is in.
//!
//! Each runs the binary, because "the same in two compilations" is not a property one
//! process can demonstrate about itself.

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
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// `nazm interface`, run from `cwd`, with whatever extra arguments are given.
fn interface_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut c = Command::new(NAZM);
    c.arg("interface").args(args).current_dir(cwd);
    c.output().expect("run nazm interface")
}

fn published(cwd: &Path, args: &[&str]) -> String {
    let out = interface_in(cwd, args);
    assert!(
        out.status.success(),
        "nazm interface {args:?} refused: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A project with two entry points over one library.
///
/// The shape §39 of the directive asks for, and the shape a real tree has: an
/// application, a tool, and something they share.
fn two_entry_points(dir: &Path) {
    write(
        dir,
        "app.nz",
        "use \"lib/common.nz\";\nfn main() -> Int { shared(1) }\n",
    );
    write(
        dir,
        "tool.nz",
        "use \"lib/common.nz\";\nfn main() -> Int { shared(2) }\n",
    );
    write(
        dir,
        "lib/common.nz",
        "pub fn shared(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 3 }\n",
    );
}

#[test]
fn a_marker_gives_a_module_one_identity_whichever_file_was_named() {
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());
    write(dir.path(), "nazm.root", "");

    // The library's own interface, published on its own.
    let alone = published(dir.path(), &["lib/common.nz"]);
    assert!(alone.contains(r#""module":"lib/common.nz""#), "{alone}");

    // And the way the two applications name it. The same key, from three compilations
    // that began at three different files.
    let from_app = published(dir.path(), &["app.nz"]);
    let from_tool = published(dir.path(), &["tool.nz"]);
    assert!(
        from_app.contains(r#""dependencies":["@core/prelude","lib/common.nz"]"#),
        "{from_app}"
    );
    assert!(
        from_tool.contains(r#""dependencies":["@core/prelude","lib/common.nz"]"#),
        "{from_tool}"
    );
}

#[test]
fn without_a_marker_the_key_still_depends_on_which_file_was_named() {
    // The N3 behaviour, unchanged and asserted rather than left implied: nothing that
    // worked before works differently, and the limitation is exactly what a declared root
    // removes. A test that only showed the new behaviour would not establish that.
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());

    let alone = published(dir.path(), &["lib/common.nz"]);
    assert!(alone.contains(r#""module":"common.nz""#), "{alone}");
    let from_app = published(dir.path(), &["app.nz"]);
    assert!(
        from_app.contains(r#""dependencies":["@core/prelude","lib/common.nz"]"#),
        "{from_app}"
    );
}

#[test]
fn a_named_root_does_what_a_marker_does() {
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());

    let alone = published(
        dir.path(),
        &[
            "lib/common.nz",
            "--source-root",
            dir.path().to_str().unwrap(),
        ],
    );
    assert!(alone.contains(r#""module":"lib/common.nz""#), "{alone}");

    // And it agrees with the marker, byte for byte: two ways of saying one thing.
    write(dir.path(), "nazm.root", "");
    assert_eq!(alone, published(dir.path(), &["lib/common.nz"]));
}

#[test]
fn the_working_directory_does_not_reach_a_key() {
    // Three shells, three spellings of one file, one answer. The absolute path is joined
    // to make the comparison possible and removed again before a key exists.
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());
    write(dir.path(), "nazm.root", "");

    let from_root = published(dir.path(), &["lib/common.nz"]);
    let from_lib = published(&dir.path().join("lib"), &["common.nz"]);
    let from_above = published(
        dir.path().parent().expect("a parent"),
        &[dir
            .path()
            .join("lib/common.nz")
            .to_str()
            .expect("utf-8 path")],
    );
    assert_eq!(from_root, from_lib);
    assert_eq!(from_root, from_above);
}

#[test]
fn two_checkouts_under_one_declared_root_agree() {
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    assert_ne!(a.path(), b.path());
    for d in [a.path(), b.path()] {
        two_entry_points(d);
        write(d, "nazm.root", "");
    }
    assert_eq!(
        published(a.path(), &["lib/common.nz"]),
        published(b.path(), &["lib/common.nz"])
    );
}

#[test]
fn the_nearest_marker_decides() {
    // A project inside a project is its own project. Whoever stands in the inner
    // directory and writes a marker meant it.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "nazm.root", "");
    write(dir.path(), "inner/nazm.root", "");
    write(dir.path(), "inner/lib/m.nz", "pub fn v() -> Int { 1 }\n");

    let out = published(dir.path(), &["inner/lib/m.nz"]);
    assert!(out.contains(r#""module":"lib/m.nz""#), "{out}");
}

#[test]
fn a_marker_with_anything_in_it_is_refused() {
    // It declares where a project begins and owns nothing else. The first field it is
    // allowed to carry is the first field of a manifest, and this language has nowhere to
    // put one — so the refusal is the mechanism that keeps it from becoming one.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "nazm.root", "version = 1\n");
    write(dir.path(), "m.nz", "pub fn v() -> Int { 1 }\n");

    let out = interface_in(dir.path(), &["m.nz"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must be empty"), "{err}");
}

#[test]
fn a_root_that_does_not_contain_the_root_module_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a/m.nz", "pub fn v() -> Int { 1 }\n");
    std::fs::create_dir_all(dir.path().join("b")).expect("b");

    let out = interface_in(
        dir.path(),
        &[
            "a/m.nz",
            "--source-root",
            dir.path().join("b").to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not inside the source root"), "{err}");
}

#[test]
fn a_module_above_the_declared_root_compiles_and_declines_a_key() {
    // §40 of the directive, and N3's principle: a limitation on durable identity is not a
    // reason to refuse a program. It runs; it simply has no key, and everything built on
    // one declines rather than guesses.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "outside.nz", "pub fn v() -> Int { 4 }\n");
    write(dir.path(), "project/nazm.root", "");
    write(
        dir.path(),
        "project/main.nz",
        "use \"../outside.nz\";\nfn main() -> Int { v() }\n",
    );

    let ran = Command::new(NAZM)
        .args(["run", dir.path().join("project/main.nz").to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "4");

    let out = interface_in(dir.path(), &["project/main.nz"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("durable identity"), "{err}");
}

#[test]
fn a_marker_changes_no_programs_meaning() {
    // The marker is a build-system fact, not a language one. Nothing a Nazm program can
    // write observes it, and adding one must not change what a program does.
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());

    let run = |where_: &Path| {
        let out = Command::new(NAZM)
            .args(["run", where_.join("app.nz").to_str().unwrap()])
            .output()
            .expect("run");
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        )
    };
    let before = run(dir.path());
    write(dir.path(), "nazm.root", "");
    assert_eq!(before, run(dir.path()));
    assert_eq!(before, (true, "4".to_owned()));
}
