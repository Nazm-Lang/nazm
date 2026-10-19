//! N56: the registry from the command line — publish, require, lock, build, yank, and the
//! refusals a build must make before it compiles anything.
//!
//! `docs/internal/architecture.md` §7.58. The model is `nazm-package`'s, tested there; this holds the
//! commands and the builds to it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(path: &Path, text: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn nazm(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A library `shapes` at `version` whose `sides` returns `answer`.
fn shapes(dir: &Path, version: &str, answer: i64) {
    write(
        &dir.join("nazm.toml"),
        &format!("schema = \"nazm.package/1\"\n\n[package]\nname = \"shapes\"\nversion = \"{version}\"\n"),
    );
    write(
        &dir.join("src/poly.nz"),
        &format!("pub fn sides(n: Int) -> Int {{ n + {answer} }}\n"),
    );
}

fn app(dir: &Path, req: &str) -> PathBuf {
    let app = dir.join("app");
    write(
        &app.join("nazm.toml"),
        &format!(
            "schema = \"nazm.package/1\"\n\n[package]\nname = \"app\"\nversion = \"1.0.0\"\nmain = \"main.nz\"\n\n\
             [registry]\npath = \"../registry\"\n\n[dependencies]\nshapes = {{ version = \"{req}\" }}\n"
        ),
    );
    write(
        &app.join("src/main.nz"),
        "use \"shapes:poly.nz\";\nfn main() -> Int { sides(1) }\n",
    );
    app
}

/// Publish `shapes` `version` (answering `answer`) through the command line.
fn publish(root: &Path, version: &str, answer: i64) {
    let src = root.join(format!("work/shapes-{version}"));
    shapes(&src, version, answer);
    let o = nazm(
        root,
        &["publish", src.to_str().unwrap(), "--registry", "registry"],
    );
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn a_registry_dependency_is_published_resolved_locked_built_and_run() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    publish(root, "1.0.0", 10);
    publish(root, "1.1.0", 20);
    publish(root, "2.0.0", 30);
    let app = app(root, "^1.0.0");
    // `^1.0.0` takes 1.1.0, the highest it admits.
    let r = nazm(root, &["run", "--interpret", "app"]);
    assert_eq!(out(&r), "21\n", "{}", err(&r));
    // Locked, then a newer 1.x published: the lockfile's choice stands, and `--locked` holds.
    let l = nazm(root, &["lock", "app"]);
    assert!(l.status.success(), "{}", err(&l));
    let lock = std::fs::read_to_string(app.join("nazm.lock")).unwrap();
    assert!(
        lock.contains("source = \"registry+shapes@1.1.0\""),
        "{lock}"
    );
    publish(root, "1.2.0", 40);
    let r = nazm(root, &["run", "--interpret", "--locked", "app"]);
    assert_eq!(out(&r), "21\n", "{}", err(&r));
    let b = nazm(root, &["build", "--locked", "app", "-o", "app.exe"]);
    assert!(b.status.success(), "{}", err(&b));
    let ran = Command::new(root.join("app.exe")).output().unwrap();
    assert_eq!(out(&ran), "21\n");
    // Yanking the locked version keeps it for the lockfile; without one, it is never chosen.
    let y = nazm(root, &["yank", "shapes@1.1.0", "--registry", "registry"]);
    assert!(y.status.success(), "{}", err(&y));
    assert_eq!(out(&nazm(root, &["run", "--interpret", "app"])), "21\n");
    std::fs::remove_file(app.join("nazm.lock")).unwrap();
    assert_eq!(out(&nazm(root, &["run", "--interpret", "app"])), "41\n");
}

#[test]
fn a_tampered_registry_is_refused_before_anything_is_compiled() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    publish(root, "1.0.0", 10);
    app(root, "^1.0.0");
    assert_eq!(out(&nazm(root, &["run", "--interpret", "app"])), "11\n");
    write(
        &root.join("registry/packages/shapes/1.0.0/src/poly.nz"),
        "pub fn sides(n: Int) -> Int { n + 999 }\n",
    );
    let r = nazm(root, &["build", "app", "-o", "app.exe"]);
    assert!(!r.status.success());
    assert!(err(&r).contains("N0507"), "{}", err(&r));
    assert!(!root.join("app.exe").exists());
}

#[test]
fn a_published_version_cannot_be_published_again_and_nothing_satisfiable_is_named() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    publish(root, "1.0.0", 10);
    let again = root.join("work/again");
    shapes(&again, "1.0.0", 11);
    let o = nazm(
        root,
        &["publish", again.to_str().unwrap(), "--registry", "registry"],
    );
    assert!(!o.status.success());
    assert!(err(&o).contains("N0512"), "{}", err(&o));
    app(root, "^3.0.0");
    let r = nazm(root, &["check", "app"]);
    assert!(!r.status.success());
    assert!(
        err(&r).contains("N0513") && err(&r).contains("`app` requires ^3.0.0"),
        "{}",
        err(&r)
    );
    let y = nazm(root, &["yank", "shapes", "--registry", "registry"]);
    assert!(!y.status.success());
    assert!(err(&y).contains("NAME@VERSION"), "{}", err(&y));
}

#[cfg(unix)]
#[test]
fn a_registry_package_holding_a_link_is_refused_by_the_build() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    publish(root, "1.0.0", 10);
    app(root, "^1.0.0");
    write(
        &root.join("secret.nz"),
        "pub fn sides(n: Int) -> Int { 0 }\n",
    );
    std::os::unix::fs::symlink(
        root.join("secret.nz"),
        root.join("registry/packages/shapes/1.0.0/src/linked.nz"),
    )
    .unwrap();
    let r = nazm(root, &["check", "app"]);
    assert!(!r.status.success());
    assert!(err(&r).contains("N0514"), "{}", err(&r));
}

#[test]
fn update_rewrites_the_lockfile_and_prints_each_change() {
    // N90, §7.91.
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    publish(root, "1.0.0", 10);
    let app = app(root, "^1.0.0");
    assert!(nazm(root, &["lock", "app"]).status.success());
    publish(root, "1.1.0", 20);
    // The lockfile still holds 1.0.0.
    assert_eq!(out(&nazm(root, &["run", "--interpret", "app"])), "11\n");
    let u = nazm(root, &["update", "app", "shapes"]);
    assert!(u.status.success(), "{}", err(&u));
    assert_eq!(out(&u), "shapes 1.0.0 → 1.1.0\n");
    assert!(std::fs::read_to_string(app.join("nazm.lock"))
        .unwrap()
        .contains("registry+shapes@1.1.0"));
    assert_eq!(out(&nazm(root, &["run", "--interpret", "app"])), "21\n");
    let again = nazm(root, &["update", "app"]);
    assert_eq!(out(&again), "no changes\n");
    let unknown = nazm(root, &["update", "app", "circles"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(err(&unknown).contains("N0501"), "{}", err(&unknown));
}
