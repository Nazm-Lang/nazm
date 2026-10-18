//! N46: packages, the lockfile, and reproducible builds.
//!
//! `docs/architecture.md` §7.48. What only whole builds show: a package built from its
//! directory under every implementation, with a diamond resolved to one package; every
//! resolution refusal by its code; a package seeing only its own dependencies; a lockfile whose
//! bytes are a function of the packages alone and that a `--locked` build is held to, refusing
//! tampering; dependency modules keyed by package; the caches reusing an unchanged dependency and
//! redoing a changed one; and two copies of one package, built from empty caches in two
//! directories, producing byte-identical executables.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn manifest(name: &str, version: &str, main: bool, deps: &[(&str, &str, &str)]) -> String {
    let mut s = format!(
        "schema = \"nazm.package/1\"\n[package]\nname = \"{name}\"\nversion = \"{version}\"\n"
    );
    if main {
        s.push_str("main = \"main.nz\"\n");
    }
    if !deps.is_empty() {
        s.push_str("[dependencies]\n");
        for (n, path, v) in deps {
            s.push_str(&format!(
                "{n} = {{ path = \"{path}\", version = \"{v}\" }}\n"
            ));
        }
    }
    s
}

/// app → geo → shapes, and app → shapes too: a diamond.
fn diamond(root: &Path) {
    write(
        root,
        "shapes/nazm.toml",
        &manifest("shapes", "0.3.1", false, &[]),
    );
    write(
        root,
        "shapes/src/poly.nz",
        "pub fn sides(n: Int) -> Int { n }\n",
    );
    write(
        root,
        "shapes/src/extra.nz",
        "pub fn unused() -> Int { 0 }\n",
    );
    write(
        root,
        "geo/nazm.toml",
        &manifest("geo", "1.2.0", false, &[("shapes", "../shapes", "0.3.1")]),
    );
    write(
        root,
        "geo/src/lib.nz",
        "use \"shapes:poly.nz\";\npub fn perimeter(n: Int, len: Int) -> Int { sides(n) * len }\n",
    );
    write(
        root,
        "app/nazm.toml",
        &manifest(
            "app",
            "0.1.0",
            true,
            &[("geo", "../geo", "1.2.0"), ("shapes", "../shapes", "0.3.1")],
        ),
    );
    write(
        root,
        "app/src/main.nz",
        "use \"geo:lib.nz\";\nuse \"shapes:poly.nz\";\nfn main() -> Int { perimeter(4, 10) + sides(2) }\n",
    );
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

#[test]
fn a_package_builds_from_its_directory_everywhere_and_a_diamond_is_one_package() {
    let d = tempfile::tempdir().unwrap();
    diamond(d.path());
    let r = nazm(d.path(), &["run", "--interpret", "app"]);
    assert_eq!(
        (r.status.code(), out(&r).as_str()),
        (Some(0), "42\n"),
        "{}",
        err(&r)
    );
    let c = nazm(d.path(), &["check", "app"]);
    assert!(c.status.success(), "{}", err(&c));
    for backend in ["llvm", "cranelift"] {
        let exe = d
            .path()
            .join(crate::common::exe_name(format!("app-{backend}")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "app",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert!(b.status.success(), "{backend}: {}", err(&b));
        let r = common::bounded_exe(&exe).output().unwrap();
        assert_eq!(out(&r), "42\n", "{backend}");
    }
    // The lockfile lists `shapes` once.
    let l = nazm(d.path(), &["lock", "app"]);
    assert!(l.status.success(), "{}", err(&l));
    let lock = std::fs::read_to_string(d.path().join("app/nazm.lock")).unwrap();
    assert_eq!(lock.matches("name = \"shapes\"").count(), 1, "{lock}");
    assert!(
        lock.contains("dependencies = [\"geo\", \"shapes\"]"),
        "{lock}"
    );
}

fn refused(setup: impl Fn(&Path), args: &[&str], code: &str) -> String {
    let d = tempfile::tempdir().unwrap();
    setup(d.path());
    let o = nazm(d.path(), args);
    assert!(!o.status.success(), "{args:?} succeeded");
    let e = err(&o);
    assert!(
        e.contains(&format!("[{code}]")) || e.contains(&format!("\"code\":\"{code}\"")),
        "{args:?}: wanted {code}: {e}"
    );
    e
}

#[test]
fn every_resolution_refusal_has_its_own_code() {
    // An unknown key, and a version of the wrong shape.
    refused(
        |r| {
            write(
                r,
                "app/nazm.toml",
                &format!("{}edition = 2\n", manifest("app", "0.1.0", true, &[])),
            );
            write(r, "app/src/main.nz", "fn main() -> Int { 0 }\n");
        },
        &["run", "--interpret", "app"],
        "N0500",
    );
    refused(
        |r| {
            write(r, "app/nazm.toml", &manifest("app", "0.1", true, &[]));
            write(r, "app/src/main.nz", "fn main() -> Int { 0 }\n");
        },
        &["run", "--interpret", "app"],
        "N0500",
    );
    // A dependency that is not there.
    refused(
        |r| {
            write(
                r,
                "app/nazm.toml",
                &manifest("app", "0.1.0", true, &[("gone", "../gone", "1.0.0")]),
            );
            write(r, "app/src/main.nz", "fn main() -> Int { 0 }\n");
        },
        &["run", "--interpret", "app"],
        "N0501",
    );
    // A declared version its manifest does not state.
    refused(
        |r| {
            diamond(r);
            write(
                r,
                "geo/nazm.toml",
                &manifest("geo", "1.2.0", false, &[("shapes", "../shapes", "0.3.2")]),
            );
        },
        &["run", "--interpret", "app"],
        "N0502",
    );
    // Two packages of one name: `shapes` at two paths.
    refused(
        |r| {
            diamond(r);
            write(
                r,
                "shapes2/nazm.toml",
                &manifest("shapes", "0.3.1", false, &[]),
            );
            write(
                r,
                "shapes2/src/poly.nz",
                "pub fn sides(n: Int) -> Int { n }\n",
            );
            write(
                r,
                "geo/nazm.toml",
                &manifest("geo", "1.2.0", false, &[("shapes", "../shapes2", "0.3.1")]),
            );
        },
        &["run", "--interpret", "app"],
        "N0503",
    );
    // A declared name the package at that path does not have.
    refused(
        |r| {
            diamond(r);
            write(
                r,
                "app/nazm.toml",
                &manifest("app", "0.1.0", true, &[("geometry", "../geo", "1.2.0")]),
            );
        },
        &["run", "--interpret", "app"],
        "N0503",
    );
    // A cycle, named in full.
    let e = refused(
        |r| {
            write(
                r,
                "a/nazm.toml",
                &manifest("a", "1.0.0", true, &[("b", "../b", "1.0.0")]),
            );
            write(r, "a/src/main.nz", "fn main() -> Int { 0 }\n");
            write(
                r,
                "b/nazm.toml",
                &manifest("b", "1.0.0", false, &[("a", "../a", "1.0.0")]),
            );
            write(r, "b/src/lib.nz", "pub fn f() -> Int { 0 }\n");
        },
        &["run", "--interpret", "a"],
        "N0504",
    );
    assert!(e.contains("a → b → a"), "{e}");
    // Reached through a package outside it, the cycle is still named alone: the path from the
    // root is not part of it.
    let e = refused(
        |r| {
            write(
                r,
                "app/nazm.toml",
                &manifest("app", "1.0.0", true, &[("a", "../a", "1.0.0")]),
            );
            write(r, "app/src/main.nz", "fn main() -> Int { 0 }\n");
            write(
                r,
                "a/nazm.toml",
                &manifest("a", "1.0.0", false, &[("b", "../b", "1.0.0")]),
            );
            write(r, "a/src/lib.nz", "pub fn f() -> Int { 0 }\n");
            write(
                r,
                "b/nazm.toml",
                &manifest("b", "1.0.0", false, &[("a", "../a", "1.0.0")]),
            );
            write(r, "b/src/lib.nz", "pub fn g() -> Int { 0 }\n");
        },
        &["run", "--interpret", "app"],
        "N0504",
    );
    assert!(e.contains("a dependency cycle: a → b → a"), "{e}");
    assert!(!e.contains("app →"), "{e}");
    // A package with no entry, and a directory that is not a package.
    refused(
        |r| {
            write(r, "lib/nazm.toml", &manifest("lib", "1.0.0", false, &[]));
            write(r, "lib/src/lib.nz", "pub fn f() -> Int { 0 }\n");
        },
        &["run", "--interpret", "lib"],
        "N0509",
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("plain")).unwrap();
    let o = nazm(d.path(), &["run", "--interpret", "plain"]);
    assert_eq!(o.status.code(), Some(3), "{}", err(&o));
}

#[test]
fn a_package_sees_its_own_dependencies_and_nothing_else() {
    // `shapes` is `geo`'s, not `app`'s: `app` may not reach it.
    let e = refused(
        |r| {
            diamond(r);
            write(
                r,
                "app/nazm.toml",
                &manifest("app", "0.1.0", true, &[("geo", "../geo", "1.2.0")]),
            );
        },
        &["check", "--json", "app"],
        "N0508",
    );
    assert!(e.contains("`geo`"), "{e}");
    // A relative import may not leave its package's source root.
    refused(
        |r| {
            diamond(r);
            write(
                r,
                "app/src/main.nz",
                "use \"../../shapes/src/poly.nz\";\nfn main() -> Int { sides(1) }\n",
            );
        },
        &["check", "app"],
        "N0205",
    );
    // And `NAME:` outside any package means nothing.
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "prog.nz",
        "use \"shapes:poly.nz\";\nfn main() -> Int { 0 }\n",
    );
    let o = nazm(d.path(), &["check", "prog.nz"]);
    assert!(err(&o).contains("N0508"), "{}", err(&o));
}

#[test]
fn the_lockfile_is_a_function_of_the_packages_and_a_locked_build_is_held_to_it() {
    let d = tempfile::tempdir().unwrap();
    diamond(d.path());
    let o = nazm(d.path(), &["run", "--interpret", "--locked", "app"]);
    assert!(err(&o).contains("[N0505]"), "{}", err(&o));
    assert!(nazm(d.path(), &["lock", "app"]).status.success());
    let lock = std::fs::read_to_string(d.path().join("app/nazm.lock")).unwrap();
    // Locked again: the same bytes, and not rewritten.
    let again = nazm(d.path(), &["lock", "app"]);
    assert!(err(&again).contains("up to date"), "{}", err(&again));
    // The same packages in another directory lock to the same bytes: nothing absolute enters.
    let e = tempfile::tempdir().unwrap();
    diamond(e.path());
    assert!(nazm(e.path(), &["lock", "app"]).status.success());
    assert_eq!(
        std::fs::read_to_string(e.path().join("app/nazm.lock")).unwrap(),
        lock
    );
    let ok = nazm(d.path(), &["run", "--interpret", "--locked", "app"]);
    assert_eq!(out(&ok), "42\n", "{}", err(&ok));
    // A file inside the package under `--locked` is held to it too.
    let ok = nazm(d.path(), &["check", "--locked", "app/src/main.nz"]);
    assert!(ok.status.success(), "{}", err(&ok));

    // Tampering with a dependency's source, or its manifest, without locking again.
    write(
        d.path(),
        "shapes/src/poly.nz",
        "pub fn sides(n: Int) -> Int { n + 1 }\n",
    );
    let t = nazm(d.path(), &["build", "--locked", "app", "-o", "x"]);
    assert!(
        err(&t).contains("[N0507]") && err(&t).contains("`shapes`"),
        "{}",
        err(&t)
    );
    assert!(
        !d.path().join(crate::common::exe_name("x")).exists(),
        "nothing was built"
    );
    write(
        d.path(),
        "shapes/src/poly.nz",
        "pub fn sides(n: Int) -> Int { n }\n",
    );
    assert!(nazm(d.path(), &["run", "--interpret", "--locked", "app"])
        .status
        .success());
    write(d.path(), "shapes/src/new.nz", "pub fn g() -> Int { 0 }\n");
    assert!(err(&nazm(d.path(), &["run", "--interpret", "--locked", "app"])).contains("[N0507]"));
    std::fs::remove_file(d.path().join("shapes/src/new.nz")).unwrap();

    // A changed dependency set or version is a different resolution.
    write(
        d.path(),
        "app/nazm.toml",
        &manifest(
            "app",
            "0.1.0",
            true,
            &[("geo", "../geo", "1.2.0"), ("shapes", "../shapes", "0.3.1")],
        )
        .replace("0.1.0", "0.2.0"),
    );
    assert!(err(&nazm(d.path(), &["run", "--interpret", "--locked", "app"])).contains("[N0506]"));
}

#[test]
fn a_dependency_s_module_is_keyed_by_its_package() {
    let d = tempfile::tempdir().unwrap();
    diamond(d.path());
    let main = d.path().join("app/src/main.nz");
    let text = std::fs::read_to_string(&main).unwrap();
    let at = text.find("perimeter(").unwrap();
    let o = nazm(
        d.path(),
        &[
            "context",
            "--root",
            main.to_str().unwrap(),
            "--file",
            main.to_str().unwrap(),
            "--byte",
            &at.to_string(),
            "--json",
        ],
    );
    let packet = out(&o);
    assert!(
        packet.contains("\"definition\":\"geo@1.2.0/lib.nz::fn perimeter\""),
        "{packet}{}",
        err(&o)
    );
}

fn report(o: &Output) -> (u64, u64) {
    let line = out(o)
        .lines()
        .find(|l| l.contains("nazm.check-report/1"))
        .unwrap_or_else(|| panic!("{}{}", out(o), err(o)))
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    (
        v["checked"].as_u64().unwrap(),
        v["reused"].as_u64().unwrap(),
    )
}

#[test]
fn an_unchanged_dependency_is_reused_and_a_changed_one_is_not() {
    let d = tempfile::tempdir().unwrap();
    diamond(d.path());
    let first = nazm(d.path(), &["check", "app", "--cache-report"]);
    let (checked, _) = report(&first);
    assert!(checked >= 3, "{}", out(&first));
    let second = nazm(d.path(), &["check", "app", "--cache-report"]);
    assert_eq!(report(&second).0, 0, "nothing changed: {}", out(&second));
    // A body-only change to `shapes`: it is checked again; its dependents' interfaces did not
    // move, so they are reused.
    write(
        d.path(),
        "shapes/src/poly.nz",
        "pub fn sides(n: Int) -> Int { n + 0 }\n",
    );
    let third = nazm(d.path(), &["check", "app", "--cache-report"]);
    assert_eq!(report(&third).0, 1, "{}", out(&third));
    let r = nazm(d.path(), &["run", "--interpret", "app"]);
    assert_eq!(out(&r), "42\n");
}

#[test]
fn two_copies_built_from_empty_caches_are_byte_identical() {
    for backend in ["llvm", "cranelift"] {
        let mut exes = Vec::new();
        for _ in 0..2 {
            let d = tempfile::tempdir().unwrap();
            diamond(d.path());
            let exe = d.path().join(crate::common::exe_name("out"));
            let b = nazm(
                d.path(),
                &[
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "app",
                    "-o",
                    exe.to_str().unwrap(),
                ],
            );
            assert!(b.status.success(), "{}", err(&b));
            exes.push(std::fs::read(&exe).unwrap());
            // Kept alive until read.
            drop(d);
        }
        assert!(
            exes[0] == exes[1],
            "{backend}: two builds of one package differ"
        );
    }
}

#[test]
fn a_large_package_graph_resolves_and_locks_deterministically() {
    // Two hundred packages, each depending on up to three earlier ones.
    let d = tempfile::tempdir().unwrap();
    let n: usize = 200;
    for i in 0..n {
        let deps: Vec<(String, String, String)> = [i / 2, i / 3, i.saturating_sub(1)]
            .into_iter()
            .filter(|&j| j < i)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|j| (format!("p{j}"), format!("../p{j}"), "1.0.0".to_owned()))
            .collect();
        let deps: Vec<(&str, &str, &str)> = deps
            .iter()
            .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
            .collect();
        write(
            d.path(),
            &format!("p{i}/nazm.toml"),
            &manifest(&format!("p{i}"), "1.0.0", i == n - 1, &deps),
        );
        let mut body = String::new();
        for (dep, _, _) in &deps {
            body.push_str(&format!("use \"{dep}:lib.nz\";\n"));
        }
        let calls: Vec<String> = deps
            .iter()
            .map(|(dep, _, _)| format!("f_{dep}()"))
            .collect();
        let sum = if calls.is_empty() {
            "1".to_owned()
        } else {
            format!("1 + {}", calls.join(" + "))
        };
        if i == n - 1 {
            body.push_str(&format!("fn main() -> Int {{ ({sum}) % 1000 }}\n"));
            write(d.path(), &format!("p{i}/src/main.nz"), &body);
        } else {
            body.push_str(&format!("pub fn f_p{i}() -> Int {{ ({sum}) % 1000 }}\n"));
            write(d.path(), &format!("p{i}/src/lib.nz"), &body);
        }
    }
    let root = format!("p{}", n - 1);
    let s = std::time::Instant::now();
    let l = nazm(d.path(), &["lock", &root]);
    let lock_ms = s.elapsed().as_millis();
    assert!(l.status.success(), "{}", err(&l));
    let lock = std::fs::read_to_string(d.path().join(&root).join("nazm.lock")).unwrap();
    assert_eq!(lock.matches("[[package]]").count(), n);
    // In name order, whatever order resolution met them in.
    let names: Vec<&str> = lock
        .lines()
        .filter_map(|l| l.strip_prefix("name = \"")?.strip_suffix('"'))
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    let c = nazm(d.path(), &["check", &root]);
    assert!(c.status.success(), "{}", err(&c));
    std::fs::remove_file(d.path().join(&root).join("nazm.lock")).unwrap();
    assert!(nazm(d.path(), &["lock", &root]).status.success());
    assert_eq!(
        std::fs::read_to_string(d.path().join(&root).join("nazm.lock")).unwrap(),
        lock
    );
    assert!(
        lock_ms < 20_000,
        "{lock_ms} ms to resolve and lock {n} packages"
    );
}

/// For `docs/performance.md`: resolution, a clean build and an incremental one of the diamond.
#[test]
#[ignore = "a measurement, run by the benchmark gate"]
fn what_packages_cost() {
    let d = tempfile::tempdir().unwrap();
    diamond(d.path());
    let time = |args: &[&str]| {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let s = std::time::Instant::now();
            assert!(nazm(d.path(), args).status.success());
            best = best.min(s.elapsed().as_secs_f64() * 1000.0);
        }
        best
    };
    println!("lock (3 packages): {:.1} ms", time(&["lock", "app"]));
    println!(
        "check, cold cache: {:.1} ms",
        time(&["check", "--no-cache", "app"])
    );
    println!("check, warm cache: {:.1} ms", time(&["check", "app"]));
    println!(
        "build --no-cache: {:.1} ms",
        time(&["build", "--no-cache", "app", "-o", "x"])
    );
    println!("build, warm: {:.1} ms", time(&["build", "app", "-o", "x"]));
    let _: PathBuf = d.path().join("x");
}
