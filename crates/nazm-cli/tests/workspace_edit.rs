//! N89: workspace editing — renaming what a project exports, and applying a plan stale-safely.
//!
//! `docs/internal/architecture.md` §7.90. What only a real workspace on disk shows: that an exported
//! function, record or variant is renamed in every module under a declared source root — those
//! the root's own compilation does not load included — and nowhere else; that the renamed workspace
//! checks and means what it meant; that a library package's API, a root nobody declared, and a
//! rename that would capture a name are refused; and that `nazm patch apply` writes a plan only to
//! files whose bytes are the plan's, all or nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let p = d.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn read(d: &Path, name: &str) -> String {
    std::fs::read_to_string(d.join(name)).unwrap()
}

const MATH: &str =
    "pub fn double(x: Int) -> Int ! {} { x * 2 }\nfn helper() -> Int ! {} { double(1) }\n";
const MAIN: &str = "use \"../util/math.nz\";\nfn main() -> Int { double(21) }\n";
const OTHER: &str =
    "use \"../util/math.nz\" as m;\npub fn quad(x: Int) -> Int ! {} { m::double(m::double(x)) }\n";

fn plan(d: &Path, byte: &str, name: &str) -> Value {
    let o = nazm(
        d,
        &[
            "patch",
            "rename",
            "--root",
            "app/main.nz",
            "--file",
            "util/math.nz",
            "--byte",
            byte,
            "--new-name",
            name,
            "--json",
        ],
    );
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)))
}

#[test]
fn an_exported_function_is_renamed_in_every_module_of_the_workspace() {
    let d = workspace(&[
        ("nazm.root", ""),
        ("util/math.nz", MATH),
        ("app/main.nz", MAIN),
        ("app/other.nz", OTHER),
        // Not in the workspace's use of `double`: its own function of that name stays.
        (
            "app/unrelated.nz",
            "fn double(x: Int) -> Int ! {} { x + x }\nfn main() -> Int { double(2) }\n",
        ),
    ]);
    let p = plan(d.path(), "7", "twice");
    assert_eq!(p["status"], "available", "{p}");
    let files: Vec<(&str, usize)> = p["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap(),
                f["edits"].as_array().unwrap().len(),
            )
        })
        .collect();
    // `app/other.nz` is in no compilation `app/main.nz` loads; the workspace finds it.
    assert_eq!(
        files,
        [("app/main.nz", 1), ("app/other.nz", 2), ("util/math.nz", 2)]
    );
    std::fs::write(
        d.path().join("plan.json"),
        serde_json::to_string(&p).unwrap(),
    )
    .unwrap();
    let a = nazm(d.path(), &["patch", "apply", "plan.json"]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert_eq!(
        read(d.path(), "util/math.nz"),
        MATH.replace("double", "twice")
    );
    assert_eq!(
        read(d.path(), "app/main.nz"),
        MAIN.replace("double", "twice")
    );
    assert_eq!(
        read(d.path(), "app/other.nz"),
        OTHER.replace("double", "twice")
    );
    assert!(read(d.path(), "app/unrelated.nz").contains("fn double"));
    // The renamed workspace checks and runs as before.
    let r = nazm(d.path(), &["run", "--interpret", "app/main.nz"]);
    assert_eq!(String::from_utf8_lossy(&r.stdout), "42\n");
}

#[test]
fn a_plan_is_applied_only_to_the_bytes_it_was_made_from() {
    let d = workspace(&[
        ("nazm.root", ""),
        ("util/math.nz", MATH),
        ("app/main.nz", MAIN),
        ("app/other.nz", OTHER),
    ]);
    let p = plan(d.path(), "7", "twice");
    std::fs::write(
        d.path().join("plan.json"),
        serde_json::to_string(&p).unwrap(),
    )
    .unwrap();
    // One file edited after planning: nothing is written, the file is named.
    let edited = format!("{OTHER}// later\n");
    std::fs::write(d.path().join("app/other.nz"), &edited).unwrap();
    let a = nazm(d.path(), &["patch", "apply", "plan.json"]);
    assert_eq!(a.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&a.stderr)
        .contains("`app/other.nz` is not the file the plan was made from"));
    assert_eq!(read(d.path(), "util/math.nz"), MATH);
    assert_eq!(read(d.path(), "app/main.nz"), MAIN);
    assert_eq!(read(d.path(), "app/other.nz"), edited);
    // A plan whose path leaves the base is refused.
    let mut escaping = p.clone();
    escaping["files"][0]["path"] = Value::from("../outside.nz");
    std::fs::write(
        d.path().join("bad.json"),
        serde_json::to_string(&escaping).unwrap(),
    )
    .unwrap();
    let b = nazm(d.path(), &["patch", "apply", "bad.json"]);
    assert_eq!(b.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&b.stderr).contains("not a path inside the base"));
}

#[test]
fn without_a_declared_workspace_or_in_a_library_package_an_export_is_not_renamed() {
    // No marker, no manifest: the root is the file's directory, and no workspace is declared.
    let d = workspace(&[
        ("math.nz", MATH),
        (
            "main.nz",
            "use \"math.nz\";\nfn main() -> Int { double(21) }\n",
        ),
    ]);
    let o = nazm(
        d.path(),
        &[
            "patch",
            "rename",
            "--root",
            "main.nz",
            "--file",
            "math.nz",
            "--byte",
            "7",
            "--new-name",
            "twice",
            "--json",
        ],
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        (v["status"].as_str(), v["reason"].as_str()),
        (Some("unavailable"), Some("exported"))
    );
    // A library package — no `main` in its manifest — has an API other packages import.
    let lib = workspace(&[
        (
            "nazm.toml",
            "schema = \"nazm.package/1\"\n\n[package]\nname = \"mathlib\"\nversion = \"1.0.0\"\n",
        ),
        ("src/math.nz", MATH),
        (
            "src/use.nz",
            "use \"math.nz\";\npub fn four() -> Int ! {} { double(2) }\n",
        ),
    ]);
    let o = nazm(
        lib.path(),
        &[
            "patch",
            "rename",
            "--root",
            "src/use.nz",
            "--file",
            "src/math.nz",
            "--byte",
            "7",
            "--new-name",
            "twice",
            "--json",
        ],
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        (v["status"].as_str(), v["reason"].as_str()),
        (Some("unavailable"), Some("package_api")),
        "{v}"
    );
    // An application package — with a `main` — is a workspace like any declared root.
    std::fs::write(
        lib.path().join("nazm.toml"),
        "schema = \"nazm.package/1\"\n\n[package]\nname = \"app\"\nversion = \"1.0.0\"\nmain = \"use.nz\"\n",
    )
    .unwrap();
    std::fs::write(
        lib.path().join("src/use.nz"),
        "use \"math.nz\";\nfn main() -> Int { double(2) }\n",
    )
    .unwrap();
    let o = nazm(
        lib.path(),
        &[
            "patch",
            "rename",
            "--root",
            "src/use.nz",
            "--file",
            "src/math.nz",
            "--byte",
            "7",
            "--new-name",
            "twice",
            "--json",
        ],
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["status"], "available", "{v}");
}

#[test]
fn a_rename_that_would_capture_a_name_in_an_importer_is_refused() {
    // `other.nz` already has its own `twice`: renaming `double` to it would make two meanings
    // of one name there.
    let d = workspace(&[
        ("nazm.root", ""),
        ("util/math.nz", MATH),
        ("app/main.nz", MAIN),
        (
            "app/other.nz",
            "use \"../util/math.nz\";\nfn twice(x: Int) -> Int ! {} { x }\npub fn quad(x: Int) -> Int ! {} { double(twice(x)) }\n",
        ),
    ]);
    let p = plan(d.path(), "7", "twice");
    assert_eq!(p["status"], "unavailable", "{p}");
    assert!(
        matches!(p["reason"].as_str(), Some("rejected" | "meaning_changed")),
        "{p}"
    );
}

#[test]
fn an_exported_record_and_its_field_are_renamed_across_modules() {
    let d = workspace(&[
        ("nazm.root", ""),
        ("shape.nz", "pub struct Point { x: Int, y: Int }\npub fn origin() -> Point ! {} { Point(x: 0, y: 0) }\n"),
        ("main.nz", "use \"shape.nz\";\nfn main() -> Int { let p = Point(x: 3, y: 4); p.x + origin().y }\n"),
    ]);
    // The field `x` (byte 19 of `shape.nz`).
    let o = nazm(
        d.path(),
        &[
            "patch",
            "rename",
            "--root",
            "main.nz",
            "--file",
            "shape.nz",
            "--byte",
            "19",
            "--new-name",
            "east",
            "--json",
        ],
    );
    let p: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(p["status"], "available", "{p}");
    std::fs::write(
        d.path().join("plan.json"),
        serde_json::to_string(&p).unwrap(),
    )
    .unwrap();
    assert!(nazm(d.path(), &["patch", "apply", "plan.json"])
        .status
        .success());
    assert!(read(d.path(), "main.nz").contains("Point(east: 3, y: 4); p.east"));
    let r = nazm(d.path(), &["run", "--interpret", "main.nz"]);
    assert_eq!(String::from_utf8_lossy(&r.stdout), "3\n");
}
