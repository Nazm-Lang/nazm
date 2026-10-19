//! The user cache (R1.5, D12; `docs/internal/architecture.md` §7.113): the runtime's and the standard
//! library's objects compiled once per user, under the project store's keys.
//!
//! Every build here names a temporary user cache through `NAZM_USER_CACHE_DIR`, or redirects
//! `HOME` and `XDG_CACHE_HOME` to temporary directories, so no test reads or writes a real one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// A program that reaches the runtime and one standard module, `@std/text`.
const PROGRAM: &str = "use \"@std/text\"\n\nfn main() {\n    println(text_repeat(\"ab\", 3))\n    println(text_starts_with(\"nazm\", \"na\"))\n}\n";

/// What it prints; `NAZM_RESULT_REPORT`, which cargo sets, adds `main`'s result after it.
const OUTPUT: &str = "ababab\ntrue\n";

/// A project in a directory of its own: a marker, and the program.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").expect("marker");
    std::fs::write(dir.path().join("main.nz"), PROGRAM).expect("program");
    dir
}

/// What one build did with the object stores.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Did {
    units: u64,
    compiled: u64,
    reused: u64,
    stored: u64,
}

/// `nazm build` in `dir`, with `user` as the user cache, reporting what it did.
fn build_with(mut c: Command, dir: &Path, out: &str, extra: &[&str]) -> Did {
    let result = c
        .args(["build", "main.nz", "-o", out, "--build-report"])
        .args(extra)
        .current_dir(dir)
        .output()
        .expect("build");
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: Value = stdout
        .lines()
        .find(|l| l.contains("nazm.build-report/1"))
        .and_then(|l| serde_json::from_str(l).ok())
        .expect("a report");
    let n = |name: &str| v["native"][name].as_u64().unwrap_or(u64::MAX);
    Did {
        units: n("units"),
        compiled: n("compiled"),
        reused: n("reused"),
        stored: n("stored"),
    }
}

fn with_user(user: &Path) -> Command {
    let mut c = Command::new(NAZM);
    c.env("NAZM_USER_CACHE_DIR", user)
        .env_remove("NAZM_CACHE_DIR");
    c
}

fn build(dir: &Path, user: &Path, extra: &[&str]) -> Did {
    build_with(with_user(user), dir, "prog", extra)
}

/// Every file under `dir`, or none when it does not exist.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn run(exe: &Path) -> String {
    let out = Command::new(exe).output().expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_second_project_reuses_the_runtime_and_the_standard_library_from_the_user_cache() {
    for backend in ["llvm", "cranelift"] {
        let user = tempfile::tempdir().expect("user cache");
        let (a, b) = (project(), project());
        let extra = ["--backend", backend];

        let first = build(a.path(), user.path(), &extra);
        assert_eq!(first.compiled, first.units, "{backend}: {first:?}");
        let shared = files(&user.path().join("native")).len() as u64;
        // The runtime, and `@std/text`'s own unit, at least.
        assert!(shared >= 2, "{backend}: {shared} shared objects");
        assert_eq!(
            files(&a.path().join(".nazm").join("native")).len() as u64 + shared,
            first.stored,
            "{backend}: every object is filed once, in one of the two stores"
        );
        // The project's own: its one module and its entry. `@std/text`'s unit and the runtime's
        // are the user cache's.
        assert_eq!(
            files(&a.path().join(".nazm").join("native")).len(),
            2,
            "{backend}: a shared unit was filed with the project"
        );

        // Another project: its own units compiled, every shared one reused from the user cache.
        let second = build(b.path(), user.path(), &extra);
        assert_eq!(second.reused, shared, "{backend}: {second:?}");
        assert_eq!(
            second.compiled,
            second.units - shared,
            "{backend}: {second:?}"
        );
        assert_eq!(
            files(&b.path().join(".nazm").join("native")).len() as u64,
            second.units - shared,
            "{backend}: the project's store holds its own objects and no shared one"
        );
        assert_eq!(
            files(&user.path().join("native")).len() as u64,
            shared,
            "{backend}: nothing of the second project's own reached the user cache"
        );
        assert!(run(&b.path().join(crate::common::exe_name("prog"))).starts_with(OUTPUT));
    }
}

#[test]
fn a_build_from_a_warm_user_cache_is_the_same_executable_as_an_uncached_one() {
    let user = tempfile::tempdir().expect("user cache");
    let (a, b) = (project(), project());
    build(a.path(), user.path(), &[]);
    let warm = build(b.path(), user.path(), &[]);
    assert!(warm.reused > 0, "{warm:?}");
    let cached =
        std::fs::read(b.path().join(crate::common::exe_name("prog"))).expect("cached build");

    // The same output name, so the linker writes the same name into the executable (N46).
    std::fs::remove_file(b.path().join(crate::common::exe_name("prog"))).expect("remove");
    build(b.path(), user.path(), &["--no-cache"]);
    let uncached =
        std::fs::read(b.path().join(crate::common::exe_name("prog"))).expect("uncached build");
    assert!(
        cached == uncached,
        "a build from the user cache is not the executable an uncached build makes"
    );
}

#[test]
fn another_optimisation_level_heap_limit_or_target_reuses_nothing_shared() {
    let user = tempfile::tempdir().expect("user cache");
    build(project().path(), user.path(), &[]);

    for (what, extra) in [
        ("-O2", vec!["--opt-level", "2"]),
        ("a heap limit", vec!["--heap-limit", "64M"]),
    ] {
        let p = project();
        let did = build(p.path(), user.path(), &extra);
        assert_eq!(
            did.reused, 0,
            "{what} reused another configuration's object: {did:?}"
        );
        assert!(
            run(&p.path().join(crate::common::exe_name("prog"))).starts_with(OUTPUT),
            "{what}"
        );
        // And each keeps its own: a second project with the same configuration reuses them.
        let again = build(project().path(), user.path(), &extra);
        assert!(again.reused > 0, "{what}: {again:?}");
    }

    // Another target, compiled to objects: nothing of the host's is reused.
    let other = if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let p = project();
    let objects = p.path().join("objects");
    let objects = objects.to_str().expect("utf-8 path");
    let did = build(
        p.path(),
        user.path(),
        &["--target", other, "--objects", objects],
    );
    assert_eq!(did.reused, 0, "{other} reused a host object: {did:?}");
    assert!(did.stored > 0, "{other}: {did:?}");
}

#[test]
fn a_heap_limit_is_never_reused_from_another_limit() {
    // The runtime's units under a heap limit do not depend on its value, and are shared; the unit
    // holding the limit does, and is not. Proved by what the program does: a megabyte fits under
    // one limit and not under the other, whichever was built first.
    const BIG: &str =
        "use \"@std/text\"\n\nfn main() {\n    println(text_repeat(\"x\", 1000000).len())\n}\n";
    let user = tempfile::tempdir().expect("user cache");
    let ends = |limit: &str| {
        let p = project();
        std::fs::write(p.path().join("main.nz"), BIG).expect("program");
        let did = build(p.path(), user.path(), &["--heap-limit", limit]);
        let out = Command::new(p.path().join(crate::common::exe_name("prog")))
            .output()
            .expect("run");
        (did, out.status.code())
    };
    let (_, roomy) = ends("1G");
    assert_eq!(roomy, Some(0));
    let (tight, code) = ends("64K");
    assert!(
        tight.reused > 0,
        "the limit-free runtime units were not shared: {tight:?}"
    );
    assert!(
        tight.compiled > 0,
        "the unit holding the limit was reused: {tight:?}"
    );
    assert_eq!(code, Some(2), "a 64K limit let a megabyte through");
    let (_, again) = ends("1G");
    assert_eq!(again, Some(0), "a 1G build took the 64K limit");
}

#[test]
fn deleting_the_user_cache_changes_nothing() {
    let user = tempfile::tempdir().expect("user cache");
    let p = project();
    build(p.path(), user.path(), &[]);
    let before = std::fs::read(p.path().join(crate::common::exe_name("prog"))).expect("built");
    let out = run(&p.path().join(crate::common::exe_name("prog")));

    std::fs::remove_dir_all(user.path().join("native")).expect("delete the user cache");
    std::fs::remove_dir_all(p.path().join(".nazm")).expect("delete the project store");
    let again = build(p.path(), user.path(), &[]);
    assert_eq!(
        again.compiled, again.units,
        "something survived the deletion"
    );
    assert!(
        std::fs::read(p.path().join(crate::common::exe_name("prog"))).expect("rebuilt") == before
    );
    assert_eq!(run(&p.path().join(crate::common::exe_name("prog"))), out);
}

#[test]
fn the_user_cache_is_under_the_home_directory_unless_a_variable_moves_it() {
    let home = tempfile::tempdir().expect("home");
    let xdg = tempfile::tempdir().expect("xdg");
    let p = project();
    let mut c = Command::new(NAZM);
    c.env_remove("NAZM_USER_CACHE_DIR")
        .env_remove("NAZM_CACHE_DIR")
        .env("HOME", home.path())
        .env("LOCALAPPDATA", xdg.path())
        .env("XDG_CACHE_HOME", xdg.path());
    build_with(c, p.path(), "prog", &[]);
    let expected = if cfg!(target_os = "macos") {
        home.path().join("Library").join("Caches").join("nazm")
    } else {
        xdg.path().join("nazm")
    };
    assert!(
        !files(&expected.join("native")).is_empty(),
        "nothing was filed in {}",
        expected.display()
    );

    // `NAZM_CACHE_DIR` alone moves the user cache too: one variable still moves everything.
    let moved = tempfile::tempdir().expect("moved");
    let home = tempfile::tempdir().expect("another home");
    let mut c = Command::new(NAZM);
    c.env_remove("NAZM_USER_CACHE_DIR")
        .env("NAZM_CACHE_DIR", moved.path())
        .env("LOCALAPPDATA", home.path())
        .env("HOME", home.path())
        .env("XDG_CACHE_HOME", home.path());
    let did = build_with(c, project().path(), "prog", &[]);
    assert_eq!(files(&moved.path().join("native")).len() as u64, did.stored);
    assert!(
        files(home.path()).is_empty(),
        "the home directory was written"
    );

    // And an empty `NAZM_USER_CACHE_DIR` turns it off: every object is the project's.
    let home = tempfile::tempdir().expect("a third home");
    let p = project();
    let mut c = Command::new(NAZM);
    c.env("NAZM_USER_CACHE_DIR", "")
        .env_remove("NAZM_CACHE_DIR")
        .env("HOME", home.path())
        .env("LOCALAPPDATA", home.path())
        .env("XDG_CACHE_HOME", home.path());
    let did = build_with(c, p.path(), "prog", &[]);
    assert_eq!(
        files(&p.path().join(".nazm").join("native")).len() as u64,
        did.stored
    );
    assert!(
        files(home.path()).is_empty(),
        "the home directory was written"
    );
}
