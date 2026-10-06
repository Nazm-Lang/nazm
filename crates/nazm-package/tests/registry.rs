//! N56: a local registry — version ranges, deterministic resolution, an immutable,
//! digest-checked store, and the attacks a package store has to refuse.
//!
//! `docs/architecture.md` §7.58. Every registry here is a temporary directory built by
//! [`nazm_package::publish`]; nothing reaches a network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_diag::codes;
use nazm_package::{publish, resolve, yank, Graph, Version};

fn write(path: &Path, text: &str) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// A package `name` `version` in `dir`, with `deps` as its `[dependencies]` lines.
fn package(dir: &Path, name: &str, version: &str, deps: &[&str], registry: Option<&str>) {
    let mut m = format!(
        "schema = \"nazm.package/1\"\n\n[package]\nname = \"{name}\"\nversion = \"{version}\"\nmain = \"main.nz\"\n"
    );
    if let Some(r) = registry {
        m.push_str(&format!("\n[registry]\npath = \"{r}\"\n"));
    }
    if !deps.is_empty() {
        m.push_str("\n[dependencies]\n");
        for d in deps {
            m.push_str(d);
            m.push('\n');
        }
    }
    write(&dir.join("nazm.toml"), &m);
    write(
        &dir.join("src/main.nz"),
        &format!("pub fn version() -> Str {{ \"{name} {version}\" }}\nfn main() -> Int {{ 0 }}\n"),
    );
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        Self { _dir: dir, root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Publish `name` `version` (with `deps`) into `reg`.
    fn publish(&self, reg: &str, name: &str, version: &str, deps: &[&str]) {
        let src = self.path(&format!("work/{name}-{version}"));
        package(&src, name, version, deps, None);
        publish(&src, &self.path(reg)).unwrap();
    }

    /// The application, depending on `deps`, using the registry at `reg`.
    fn app(&self, deps: &[&str], reg: &str) -> PathBuf {
        let app = self.path("app");
        package(&app, "app", "1.0.0", deps, Some(&format!("../{reg}")));
        app
    }
}

fn chosen(g: &Graph) -> Vec<String> {
    g.packages
        .iter()
        .map(|p| format!("{} {} {}", p.name, p.version, p.source))
        .collect()
}

fn refused(dir: &Path) -> nazm_diag::Code {
    let e = resolve(dir).unwrap_err();
    e.first().unwrap().code.clone()
}

// --- choosing ------------------------------------------------------------------------------

#[test]
fn the_highest_version_satisfying_every_requirement_is_chosen() {
    let w = World::new();
    for v in ["0.9.0", "1.0.0", "1.2.0", "1.4.1", "2.0.0"] {
        w.publish("reg", "shapes", v, &[]);
    }
    let app = w.app(&["shapes = { version = \"^1.1.0\" }"], "reg");
    let g = resolve(&app).unwrap();
    assert_eq!(
        chosen(&g),
        ["app 1.0.0 path+.", "shapes 1.4.1 registry+shapes@1.4.1"]
    );
    // `~` keeps the minor; `=` and a bare version are exact.
    package(
        &app,
        "app",
        "1.0.0",
        &["shapes = { version = \"~1.2.0\" }"],
        Some("../reg"),
    );
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.2.0 registry+shapes@1.2.0"
    );
    package(
        &app,
        "app",
        "1.0.0",
        &["shapes = { version = \"1.0.0\" }"],
        Some("../reg"),
    );
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.0.0 registry+shapes@1.0.0"
    );
}

#[test]
fn a_transitive_requirement_narrows_the_choice_and_a_conflict_is_refused_naming_both() {
    let w = World::new();
    for v in ["1.0.0", "1.1.0", "1.2.0"] {
        w.publish("reg", "core", v, &[]);
    }
    w.publish("reg", "geo", "2.0.0", &["core = { version = \"~1.1.0\" }"]);
    let app = w.app(
        &[
            "core = { version = \"^1.0.0\" }",
            "geo = { version = \"^2.0.0\" }",
        ],
        "reg",
    );
    let g = resolve(&app).unwrap();
    // `^1.0.0` alone would take 1.2.0; `geo`'s `~1.1.0` holds it at 1.1.0.
    assert!(
        chosen(&g).contains(&"core 1.1.0 registry+core@1.1.0".to_owned()),
        "{:?}",
        chosen(&g)
    );
    let geo = g.packages.iter().find(|p| p.name == "geo").unwrap();
    assert!(geo.dependencies.contains_key("core"));

    package(
        &app,
        "app",
        "1.0.0",
        &[
            "core = { version = \"~1.2.0\" }",
            "geo = { version = \"^2.0.0\" }",
        ],
        Some("../reg"),
    );
    let e = resolve(&app).unwrap_err();
    let d = e.first().unwrap();
    assert_eq!(d.code, codes::VERSION_UNSATISFIABLE);
    assert!(
        d.message.contains("`core`")
            && d.message.contains("`app` requires ~1.2.0")
            && d.message.contains("`geo` 2.0.0 requires ~1.1.0"),
        "{}",
        d.message
    );
}

#[test]
fn resolution_is_the_same_for_reordered_index_entries() {
    let w = World::new();
    for v in ["1.0.0", "1.3.0", "1.1.0"] {
        w.publish("reg", "shapes", v, &[]);
    }
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    let before = resolve(&app).unwrap().lock_text();
    // The index rewritten with its entries reversed: the same metadata, another order.
    let index = w.path("reg/index/shapes.toml");
    let text = std::fs::read_to_string(&index).unwrap();
    let (head, rest) = text.split_once("\n[[version]]").unwrap();
    let mut entries: Vec<&str> = rest.split("\n[[version]]").collect();
    entries.reverse();
    write(
        &index,
        &format!("{head}\n[[version]]{}", entries.join("\n[[version]]")),
    );
    assert_ne!(std::fs::read_to_string(&index).unwrap(), text);
    assert_eq!(resolve(&app).unwrap().lock_text(), before);
}

#[test]
fn where_the_registry_is_does_not_enter_the_lockfile() {
    let w = World::new();
    w.publish("reg-a", "shapes", "1.0.0", &[]);
    w.publish("reg-b", "shapes", "1.0.0", &[]);
    // The application's own manifest names the registry, so its digest moves with it; the
    // registry package's identity — name, version, source and digest — does not.
    let shapes = |g: &Graph| {
        let p = g.packages.iter().find(|p| p.name == "shapes").unwrap();
        (p.version, p.source.clone(), p.digest.clone())
    };
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg-a");
    let a = shapes(&resolve(&app).unwrap());
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg-b");
    assert_eq!(shapes(&resolve(&app).unwrap()), a);
    assert_eq!(a.1, "registry+shapes@1.0.0");
}

// --- the lockfile ---------------------------------------------------------------------------

#[test]
fn a_locked_version_is_kept_after_a_newer_one_is_published() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    let g = resolve(&app).unwrap();
    nazm_package::write_lock(&g).unwrap();
    w.publish("reg", "shapes", "1.5.0", &[]);
    // The lockfile is a preference: still satisfied, so nothing new is chosen …
    let g = resolve(&app).unwrap();
    assert_eq!(chosen(&g)[1], "shapes 1.0.0 registry+shapes@1.0.0");
    nazm_package::verify_locked(&g).unwrap();
    // … and without it, the newest is.
    std::fs::remove_file(app.join("nazm.lock")).unwrap();
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.5.0 registry+shapes@1.5.0"
    );
}

#[test]
fn a_stale_lockfile_is_refused_under_locked() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    w.publish("reg", "shapes", "2.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    nazm_package::write_lock(&resolve(&app).unwrap()).unwrap();
    // The manifest moves on; the lock still records 1.0.0, which no longer satisfies it.
    package(
        &app,
        "app",
        "1.0.0",
        &["shapes = { version = \"^2.0.0\" }"],
        Some("../reg"),
    );
    let g = resolve(&app).unwrap();
    let e = nazm_package::verify_locked(&g).unwrap_err();
    assert!(
        e.iter()
            .any(|d| d.code == codes::LOCK_MISMATCH
                && d.message.contains("`shapes` resolves to 2.0.0")),
        "{e:?}"
    );
}

#[test]
fn a_yanked_version_is_never_chosen_anew_and_a_locked_one_stands() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    w.publish("reg", "shapes", "1.1.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    nazm_package::write_lock(&resolve(&app).unwrap()).unwrap();
    yank(
        &w.path("reg"),
        "shapes",
        Version::parse("1.1.0").unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.1.0 registry+shapes@1.1.0"
    );
    std::fs::remove_file(app.join("nazm.lock")).unwrap();
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.0.0 registry+shapes@1.0.0"
    );
    yank(
        &w.path("reg"),
        "shapes",
        Version::parse("1.1.0").unwrap(),
        false,
    )
    .unwrap();
    assert_eq!(
        chosen(&resolve(&app).unwrap())[1],
        "shapes 1.1.0 registry+shapes@1.1.0"
    );
}

// --- the store, and what it refuses -----------------------------------------------------------

#[test]
fn a_published_version_never_changes() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let src = w.path("work/shapes-again");
    package(&src, "shapes", "1.0.0", &[], None);
    write(&src.join("src/extra.nz"), "fn more() -> Int { 1 }\n");
    let e = publish(&src, &w.path("reg")).unwrap_err();
    assert_eq!(e.code, codes::REGISTRY_INVALID);
    // A package that depends on a path cannot be published: the path means nothing elsewhere.
    let local = w.path("work/local");
    package(
        &local,
        "local",
        "1.0.0",
        &["shapes = { path = \"../x\", version = \"1.0.0\" }"],
        None,
    );
    assert_eq!(
        publish(&local, &w.path("reg")).unwrap_err().code,
        codes::MANIFEST_INVALID
    );
}

#[test]
fn a_tampered_package_is_refused_by_its_digest() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    resolve(&app).unwrap();
    write(
        &w.path("reg/packages/shapes/1.0.0/src/main.nz"),
        "pub fn version() -> Str { \"evil\" }\nfn main() -> Int { 0 }\n",
    );
    assert_eq!(refused(&app), codes::DIGEST_MISMATCH);
    // A file added beside the sources is tampering too.
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    write(
        &w.path("reg/packages/shapes/1.0.0/src/injected.nz"),
        "fn x() -> Int { 0 }\n",
    );
    assert_eq!(refused(&app), codes::DIGEST_MISMATCH);
}

#[cfg(unix)]
#[test]
fn a_registry_package_holding_a_link_is_refused() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    write(&w.path("secret.txt"), "not yours");
    std::os::unix::fs::symlink(
        w.path("secret.txt"),
        w.path("reg/packages/shapes/1.0.0/src/escape.nz"),
    )
    .unwrap();
    assert_eq!(refused(&app), codes::PACKAGE_UNSAFE);
}

#[test]
fn a_malformed_or_equivocating_index_is_refused() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let app = w.app(&["shapes = { version = \"^1.0.0\" }"], "reg");
    let index = w.path("reg/index/shapes.toml");
    let good = std::fs::read_to_string(&index).unwrap();
    let entry = good.split_once("\n[[version]]").unwrap().1;
    for (bad, why) in [
        (format!("{good}\n[[version]]{entry}"), "a version twice"),
        (
            good.replace("nazm.registry-index/1", "nazm.registry-index/2"),
            "another schema",
        ),
        (
            good.replace("name = \"shapes\"", "name = \"other\""),
            "another package",
        ),
        (
            good.replace("blake3:", "sha1:"),
            "a digest of another shape",
        ),
        (
            good.replace("version = \"1.0.0\"", "version = \"../../1.0.0\""),
            "a path as a version",
        ),
    ] {
        write(&index, &bad);
        assert_eq!(refused(&app), codes::REGISTRY_INVALID, "{why}");
    }
    // An index whose digest is another publish's: equivocation is a digest mismatch.
    let other = "blake3:".to_owned() + &"0".repeat(64);
    let d = good
        .split("digest = \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    write(&index, &good.replace(d, &other));
    assert_eq!(refused(&app), codes::DIGEST_MISMATCH);
}

#[test]
fn a_name_is_never_a_path_and_a_missing_registry_is_named() {
    let w = World::new();
    let app = w.path("app");
    package(
        &app,
        "app",
        "1.0.0",
        &["\"../evil\" = { version = \"^1.0.0\" }"],
        Some("../reg"),
    );
    assert_eq!(refused(&app), codes::MANIFEST_INVALID);
    package(
        &app,
        "app",
        "1.0.0",
        &["shapes = { version = \"^1.0.0\" }"],
        None,
    );
    let e = resolve(&app).unwrap_err();
    assert!(e.first().unwrap().message.contains("names no `[registry]`"));
    package(
        &app,
        "app",
        "1.0.0",
        &["shapes = { version = \"^1.0.0\" }"],
        Some("../nowhere"),
    );
    assert_eq!(refused(&app), codes::PACKAGE_NOT_FOUND);
}

#[test]
fn a_cycle_through_the_registry_is_refused() {
    let w = World::new();
    w.publish("reg", "a", "1.0.0", &["b = { version = \"^1.0.0\" }"]);
    w.publish("reg", "b", "1.0.0", &["a = { version = \"^1.0.0\" }"]);
    let app = w.app(&["a = { version = \"^1.0.0\" }"], "reg");
    assert_eq!(refused(&app), codes::PACKAGE_CYCLE);
}

#[test]
fn a_path_package_and_a_registry_dependency_of_one_name_conflict() {
    let w = World::new();
    w.publish("reg", "shapes", "1.0.0", &[]);
    let local = w.path("shapes");
    package(&local, "shapes", "1.0.0", &[], None);
    w.publish(
        "reg",
        "geo",
        "1.0.0",
        &["shapes = { version = \"^1.0.0\" }"],
    );
    let app = w.app(
        &[
            "shapes = { path = \"../shapes\", version = \"1.0.0\" }",
            "geo = { version = \"^1.0.0\" }",
        ],
        "reg",
    );
    assert_eq!(refused(&app), codes::PACKAGE_CONFLICT);
}

// --- backtracking (N90, §7.91) ---------------------------------------------------------------

#[test]
fn an_older_version_is_chosen_when_the_newest_leaves_another_package_no_version() {
    // Every `b 1.x` but 1.0.0 requires `c ^2`; the application requires `c ^1`.
    let w = World::new();
    w.publish("reg", "b", "1.0.0", &["c = { version = \"^1.0.0\" }"]);
    for v in ["1.1.0", "1.2.0"] {
        w.publish("reg", "b", v, &["c = { version = \"^2.0.0\" }"]);
    }
    for v in ["1.0.0", "1.5.0", "2.0.0"] {
        w.publish("reg", "c", v, &[]);
    }
    let app = w.app(
        &[
            "b = { version = \"^1.0.0\" }",
            "c = { version = \"^1.0.0\" }",
        ],
        "reg",
    );
    let g = resolve(&app).unwrap();
    assert_eq!(
        chosen(&g),
        [
            "app 1.0.0 path+.",
            "b 1.0.0 registry+b@1.0.0",
            "c 1.5.0 registry+c@1.5.0"
        ]
    );
    // The same answer however often it is asked.
    assert_eq!(resolve(&app).unwrap().lock_text(), g.lock_text());
}

#[test]
fn an_unsatisfiable_set_is_explained_with_every_requirement_and_who_placed_it() {
    let w = World::new();
    for v in ["1.0.0", "2.0.0"] {
        w.publish("reg", "b", v, &[]);
    }
    w.publish("reg", "c", "1.0.0", &["b = { version = \"^1.0.0\" }"]);
    let app = w.app(
        &[
            "b = { version = \"=2.0.0\" }",
            "c = { version = \"^1.0.0\" }",
        ],
        "reg",
    );
    let e = resolve(&app).unwrap_err();
    let d = e.first().unwrap();
    assert_eq!(d.code, codes::VERSION_UNSATISFIABLE);
    assert!(
        d.message.contains("`b`")
            && d.message.contains("`app` requires =2.0.0")
            && d.message.contains("`c` 1.0.0 requires ^1.0.0"),
        "{}",
        d.message
    );
}

#[test]
fn a_search_past_its_bound_says_so_rather_than_claiming_no_answer() {
    // Five packages of ten versions each, and a sixth no version of which exists: every one of
    // the 100,000 assignments of the first five dead-ends at the sixth.
    let w = World::new();
    let mut deps = Vec::new();
    for p in ["a", "b", "c", "d", "e"] {
        for minor in 0..10 {
            w.publish("reg", p, &format!("1.{minor}.0"), &[]);
        }
        deps.push(format!("{p} = {{ version = \"^1.0.0\" }}"));
    }
    w.publish("reg", "f", "1.0.0", &[]);
    deps.push("f = { version = \"^2.0.0\" }".to_owned());
    let deps: Vec<&str> = deps.iter().map(String::as_str).collect();
    let app = w.app(&deps, "reg");
    let e = resolve(&app).unwrap_err();
    let d = e.first().unwrap();
    assert_eq!(d.code, codes::VERSION_UNSATISFIABLE);
    assert!(
        d.message.contains("stopped after 10000 candidate versions"),
        "{}",
        d.message
    );
}

#[test]
fn update_drops_the_lockfiles_preference_only_for_the_packages_named() {
    let w = World::new();
    for p in ["b", "c"] {
        w.publish("reg", p, "1.0.0", &[]);
    }
    let app = w.app(
        &[
            "b = { version = \"^1.0.0\" }",
            "c = { version = \"^1.0.0\" }",
        ],
        "reg",
    );
    nazm_package::write_lock(&resolve(&app).unwrap()).unwrap();
    for p in ["b", "c"] {
        w.publish("reg", p, "1.1.0", &[]);
    }
    // The lockfile holds both while they satisfy.
    let held = chosen(&resolve(&app).unwrap());
    assert!(
        held.contains(&"b 1.0.0 registry+b@1.0.0".to_owned()),
        "{held:?}"
    );
    let one = chosen(&nazm_package::resolve_updating(&app, &["b".to_owned()]).unwrap());
    assert!(
        one.contains(&"b 1.1.0 registry+b@1.1.0".to_owned()),
        "{one:?}"
    );
    assert!(
        one.contains(&"c 1.0.0 registry+c@1.0.0".to_owned()),
        "{one:?}"
    );
    let all = chosen(&nazm_package::resolve_updating(&app, &[]).unwrap());
    assert!(
        all.contains(&"c 1.1.0 registry+c@1.1.0".to_owned()),
        "{all:?}"
    );
    let e = nazm_package::resolve_updating(&app, &["nope".to_owned()]).unwrap_err();
    assert_eq!(e.first().unwrap().code, codes::PACKAGE_NOT_FOUND);
}

#[test]
fn a_candidate_must_satisfy_every_requirement_already_placed_on_it() {
    // `base` sorts first, so it is chosen first and places `~1.1.0` on `core` before `core` is
    // chosen; the app places `~1.2.0`. No version satisfies both, and the search knows it at
    // once: it tries `base` and no version of `core` at all. A filter that admitted a version
    // satisfying *any* requirement would try two of `core`'s and refuse the same — N108 found
    // nothing held the difference.
    let w = World::new();
    for v in ["1.0.0", "1.1.0", "1.2.0"] {
        w.publish("reg", "core", v, &[]);
    }
    w.publish("reg", "base", "1.0.0", &["core = { version = \"~1.1.0\" }"]);
    let app = w.app(
        &[
            "base = { version = \"^1.0.0\" }",
            "core = { version = \"~1.2.0\" }",
        ],
        "reg",
    );
    let e = resolve(&app).unwrap_err();
    let d = e.first().unwrap();
    assert_eq!(d.code, codes::VERSION_UNSATISFIABLE);
    assert!(
        d.message.contains("(after trying 1 candidate version)"),
        "{}",
        d.message
    );
}
