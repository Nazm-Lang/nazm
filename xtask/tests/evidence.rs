//! N73: an evidence bundle's index and the compiler's bill of materials (`docs/internal/architecture.md`
//! §7.75). What the command shows: the index names every file with a digest anyone can recompute,
//! the same bundle indexes the same way twice, and verifying refuses a changed, a missing and an
//! unindexed file, each by name; the bill of materials is the lockfile's crates and the pin.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const XTASK: &str = env!("CARGO_BIN_EXE_xtask");

fn xtask(args: &[&str], dir: &Path) -> Output {
    Command::new(XTASK).args(args).arg(dir).output().unwrap()
}

fn bundle() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.prov.json"),
        "{\"schema\":\"nazm.provenance/1\"}\n",
    )
    .unwrap();
    std::fs::create_dir(d.path().join("gates")).unwrap();
    std::fs::write(d.path().join("gates/tests.log"), "test result: ok.\n").unwrap();
    std::fs::write(d.path().join("gates/mutate-1.log"), "caught 1\n").unwrap();
    std::fs::write(d.path().join("prog"), [0x7f, b'E', b'L', b'F']).unwrap();
    d
}

fn index(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap()
}

#[test]
fn the_index_names_every_file_with_its_digest_and_kind() {
    let d = bundle();
    assert!(xtask(&["evidence"], d.path()).status.success());
    let v = index(d.path());
    assert_eq!(v["schema"], "nazm.evidence/1");
    let files = v["files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "gates/mutate-1.log",
            "gates/tests.log",
            "p.prov.json",
            "prog",
            "sbom.json"
        ]
    );
    let of = |p: &str| files.iter().find(|f| f["path"] == p).unwrap().clone();
    assert_eq!(
        of("prog")["digest"],
        format!(
            "blake3:{}",
            blake3::hash(&[0x7f, b'E', b'L', b'F']).to_hex()
        )
    );
    assert_eq!(of("prog")["size"], 4);
    let kinds: Vec<&str> = paths
        .iter()
        .map(|p| {
            files.iter().find(|f| f["path"] == *p).unwrap()["kind"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(
        kinds,
        ["mutation", "tests", "provenance", "artefact", "sbom"]
    );
    // Twice, the same bytes: nothing of the time or the directory.
    let first = std::fs::read(d.path().join("index.json")).unwrap();
    assert!(xtask(&["evidence"], d.path()).status.success());
    assert_eq!(std::fs::read(d.path().join("index.json")).unwrap(), first);
    assert!(!String::from_utf8(first)
        .unwrap()
        .contains(d.path().to_str().unwrap()));
    assert!(xtask(&["evidence", "--verify"], d.path()).status.success());
}

#[test]
fn verifying_refuses_a_changed_a_missing_and_an_unindexed_file_by_name() {
    let d = bundle();
    assert!(xtask(&["evidence"], d.path()).status.success());
    std::fs::write(d.path().join("prog"), b"other").unwrap();
    std::fs::remove_file(d.path().join("gates/tests.log")).unwrap();
    std::fs::write(d.path().join("late.txt"), b"added after").unwrap();
    let o = xtask(&["evidence", "--verify"], d.path());
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("changed: prog"), "{e}");
    assert!(e.contains("missing: gates/tests.log"), "{e}");
    assert!(e.contains("not indexed: late.txt"), "{e}");
    assert!(!e.contains("p.prov.json"), "{e}");
}

#[test]
fn the_bill_of_materials_is_the_lockfiles_crates_and_the_pin() {
    let d = bundle();
    assert!(xtask(&["evidence"], d.path()).status.success());
    let s: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("sbom.json")).unwrap())
            .unwrap();
    assert_eq!(s["schema"], "nazm.sbom/1");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let lock = std::fs::read(root.join("Cargo.lock")).unwrap();
    assert_eq!(
        s["lockfile_digest"],
        format!("blake3:{}", blake3::hash(&lock).to_hex())
    );
    assert_eq!(s["rust_toolchain"], "1.98.1");
    let crates = s["crates"].as_array().unwrap();
    let count = String::from_utf8_lossy(&lock)
        .matches("[[package]]")
        .count();
    assert_eq!(crates.len(), count);
    let one = |n: &str| crates.iter().find(|c| c["name"] == n).unwrap().clone();
    assert_eq!(one("nazm-cli")["source"], "workspace");
    // Each crate at the version the lockfile pins.
    let text = String::from_utf8_lossy(&lock).into_owned();
    let pinned = text
        .split("[[package]]")
        .find(|p| p.contains("name = \"blake3\""))
        .unwrap();
    let version = pinned
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .unwrap()
        .trim_matches('"');
    assert_eq!(one("blake3")["version"], version);
    assert!(one("blake3")["checksum"].as_str().unwrap().len() == 64);
    assert!(one("blake3")["source"]
        .as_str()
        .unwrap()
        .starts_with("registry+"));
}
