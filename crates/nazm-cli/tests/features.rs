//! Gate 2: package features (`docs/general-purpose.md` §22, `docs/spec.md` *Package features*).
//!
//! What this holds: a package's `default` features are on unless nothing asks; a dependent asks a
//! dependency for more; a feature enables others, its own or a dependency's; `--features` asks
//! the root; each module's `@cfg(feature = …)` sees only its own package's set; the interpreter
//! and a native build agree; an unknown feature is refused before anything is checked; and the
//! lockfile records the enabled sets as `nazm.lock/2`, a build without features keeping `/1`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn nazm(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `shapes` declares `round` (on by default), `fast` and `unused`; `app` asks
/// `shapes` for `fast`, and declares `verbose`, which nobody enables by default.
fn packages(root: &Path) {
    write(
        root,
        "shapes/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n\
         [features]\ndefault = [\"round\"]\nround = []\nfast = []\nunused = []\n",
    );
    write(
        root,
        "shapes/src/poly.nz",
        "@cfg(feature = \"fast\")\npub fn speed() -> Int { 2 }\n\n\
         @cfg(not(feature = \"fast\"))\npub fn speed() -> Int { 1 }\n\n\
         @cfg(all(feature = \"round\", not(feature = \"unused\")))\npub fn round() -> Int { 30 }\n\n\
         @cfg(not(all(feature = \"round\", not(feature = \"unused\"))))\npub fn round() -> Int { 0 }\n",
    );
    write(
        root,
        "app/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"1.0.0\"\nmain = \"main.nz\"\n\
         [dependencies]\nshapes = { path = \"../shapes\", version = \"0.1.0\", features = [\"fast\"] }\n\
         [features]\nverbose = []\nall = [\"verbose\", \"shapes/unused\"]\n",
    );
    write(
        root,
        "app/src/main.nz",
        "use \"shapes:poly.nz\";\n\n\
         @cfg(feature = \"verbose\")\nfn tag() -> Int { 400 }\n\n\
         @cfg(not(feature = \"verbose\"))\nfn tag() -> Int { 0 }\n\n\
         // `fast` is `shapes`'s, not this package's: never on here.\n\
         @cfg(feature = \"fast\")\nfn leaked() -> Int { 5000 }\n\n\
         @cfg(not(feature = \"fast\"))\nfn leaked() -> Int { 0 }\n\n\
         fn main() -> Int { speed() + round() + tag() + leaked() }\n",
    );
}

fn answer(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_owned()
}

#[test]
fn each_package_sees_its_own_features_in_every_implementation() {
    let d = tempfile::tempdir().unwrap();
    packages(d.path());
    let plain = nazm(d.path(), &["run", "app/src/main.nz"]);
    assert_eq!(answer(&plain), "32", "{}", text(&plain));
    let verbose = nazm(
        d.path(),
        &["run", "--features", "verbose", "app/src/main.nz"],
    );
    assert_eq!(answer(&verbose), "432", "{}", text(&verbose));
    let all = nazm(d.path(), &["run", "--features", "all", "app/src/main.nz"]);
    assert_eq!(answer(&all), "402", "{}", text(&all));
    let exe = d.path().join("app-all");
    let built = nazm(
        d.path(),
        &[
            "build",
            "--features",
            "all",
            "app/src/main.nz",
            "-o",
            exe.to_str().unwrap(),
        ],
    );
    assert!(built.status.success(), "{}", text(&built));
    let ran = Command::new(&exe).output().unwrap();
    assert_eq!(answer(&ran), "402");
    let checked = nazm(
        d.path(),
        &["check", "--features", "verbose,all", "app/src/main.nz"],
    );
    assert!(checked.status.success(), "{}", text(&checked));
}

#[test]
fn a_feature_nobody_declares_is_refused_before_anything_is_checked() {
    let d = tempfile::tempdir().unwrap();
    packages(d.path());
    let r = nazm(d.path(), &["run", "--features", "nope", "app/src/main.nz"]);
    assert_eq!(r.status.code(), Some(1), "{}", text(&r));
    assert!(
        text(&r).contains("N0500") && text(&r).contains("`nope`"),
        "{}",
        text(&r)
    );
    write(d.path(), "solo.nz", "fn main() -> Int { 0 }\n");
    let outside = nazm(d.path(), &["check", "--features", "x", "solo.nz"]);
    assert_eq!(outside.status.code(), Some(5), "{}", text(&outside));
    // A manifest whose feature enables something that is not one.
    write(
        d.path(),
        "shapes/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n\
         [features]\nfast = [\"turbo\"]\n",
    );
    let bad = nazm(d.path(), &["check", "app/src/main.nz"]);
    assert!(!bad.status.success());
    assert!(
        text(&bad).contains("N0500") && text(&bad).contains("`turbo`"),
        "{}",
        text(&bad)
    );
}

#[test]
fn the_lockfile_records_enabled_features_and_a_featureless_one_stays_version_one() {
    let d = tempfile::tempdir().unwrap();
    packages(d.path());
    let l = nazm(d.path(), &["lock", "app"]);
    assert!(l.status.success(), "{}", text(&l));
    let lock = std::fs::read_to_string(d.path().join("app/nazm.lock")).unwrap();
    assert!(lock.contains("schema = \"nazm.lock/2\""), "{lock}");
    assert!(lock.contains("features = [\"fast\", \"round\"]"), "{lock}");
    assert!(lock.contains("name = \"app\""), "{lock}");
    let locked = nazm(d.path(), &["check", "--locked", "app/src/main.nz"]);
    assert!(locked.status.success(), "{}", text(&locked));

    // `shapes` no longer turns `round` on by default: the recorded set is stale.
    write(
        d.path(),
        "shapes/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n\
         [features]\nround = []\nfast = []\nunused = []\n",
    );
    let stale = nazm(d.path(), &["check", "--locked", "app/src/main.nz"]);
    assert!(!stale.status.success());
    assert!(text(&stale).contains("N0506"), "{}", text(&stale));
    assert!(
        text(&stale).contains(
            "`shapes` resolves with the features [fast], and `nazm.lock` records [fast, round]"
        ),
        "{}",
        text(&stale)
    );

    // Without a feature anywhere, the lockfile is a `nazm.lock/1`, as before Gate 2.
    let plain = tempfile::tempdir().unwrap();
    write(
        plain.path(),
        "p/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"p\"\nversion = \"1.0.0\"\nmain = \"main.nz\"\n",
    );
    write(plain.path(), "p/src/main.nz", "fn main() -> Int { 0 }\n");
    assert!(nazm(plain.path(), &["lock", "p"]).status.success());
    let lock = std::fs::read_to_string(plain.path().join("p/nazm.lock")).unwrap();
    assert!(lock.contains("schema = \"nazm.lock/1\""), "{lock}");
    assert!(!lock.contains("features"), "{lock}");
}
