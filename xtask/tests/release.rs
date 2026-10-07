//! The release's build-input digest, `scripts/source-digest.sh` (R1).
//!
//! A candidate's `PROVENANCE.txt` carries this digest so that the candidate can be checked
//! against a checkout of its commit without trusting git or the build. That is only worth
//! anything if it covers what the build reads: until R1 it covered six file extensions under
//! nine directories, and a changed `Cargo.lock`, toolchain pin, Dockerfile, mutation catalogue,
//! `.expected` fixture or linker script did not move it. These tests hold the two properties
//! that matter: every input moves it, and the set it covers is the set git tracks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn digest(root: &Path, list: bool) -> String {
    let script = repo_root().join("scripts/source-digest.sh");
    let mut c = Command::new("sh");
    c.arg(&script).arg("--root").arg(root);
    if list {
        c.arg("--list");
    }
    let out = c.output().expect("run source-digest.sh");
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).expect("utf-8")
}

/// One of each kind of build input, at the path the repository keeps it.
const INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    "docker/contained.Dockerfile",
    "xtask/mutations/mutations.toml",
    "compiler/conformance/case.expected",
    "compiler/emit.nz",
    "crates/nazm-runtime/src/core.rs",
    "crates/nazm-cli/templates/embedded/link.ld",
    "library/std/text.nz",
    "schema/nazm.lock-1.json",
    "scripts/release-candidate.sh",
    "fuzz/corpus/front/0001.txt",
    "docs/spec.md",
];

#[test]
fn every_build_input_moves_the_digest_and_nothing_git_ignores_does() {
    let dir = tempfile::tempdir().expect("tree");
    let root = dir.path();
    for (i, f) in INPUTS.iter().enumerate() {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("input {i}\n")).unwrap();
    }
    let base = digest(root, false);
    assert_eq!(base.trim().len(), 64, "{base}");

    for f in INPUTS {
        let p = root.join(f);
        let before = std::fs::read(&p).unwrap();
        std::fs::write(&p, b"tampered\n").unwrap();
        assert_ne!(
            digest(root, false),
            base,
            "changing {f} did not move the digest"
        );
        std::fs::write(&p, &before).unwrap();
        assert_eq!(
            digest(root, false),
            base,
            "restoring {f} did not restore it"
        );
    }

    // A rename is a change, though every byte is the same.
    let from = root.join("compiler/emit.nz");
    let to = root.join("compiler/emit2.nz");
    std::fs::rename(&from, &to).unwrap();
    assert_ne!(
        digest(root, false),
        base,
        "a rename did not move the digest"
    );
    std::fs::rename(&to, &from).unwrap();

    // What .gitignore excludes, and what lies outside every tracked entry, moves nothing.
    for ignored in [
        "crates/nazm-cli/.nazm/check/entry",
        "examples/.DS_Store",
        "crates/nazm-core/src/lib.rs.bk",
        "bench/record.json",
        "fuzz/Cargo.lock",
        "fuzz/target/release/fuzz",
        "review/m1d/notes.md",
        "review/m1e-review.tar.gz",
        "target/release/nazm",
        "bench-runs/run.json",
    ] {
        let p = root.join(ignored);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"local\n").unwrap();
        assert_eq!(digest(root, false), base, "{ignored} moved the digest");
    }
}

/// The digest's file set is git's: every tracked file and every untracked one git would not
/// ignore — at release time, with the tree clean, exactly the tracked files.
#[test]
fn the_digest_covers_exactly_the_files_git_sees() {
    let root = repo_root();
    let git = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&root)
        .output();
    let git = match git {
        Ok(o) if o.status.success() => o,
        // The contained image has no git, by design. Said, not hidden: the test is skipped
        // there and runs on every host and in CI.
        other => {
            eprintln!(
                "SKIPPED: git is not available here ({other:?}); the coverage check needs it"
            );
            return;
        }
    };
    let tracked: BTreeSet<String> = String::from_utf8(git.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    let covered: BTreeSet<String> = digest(&root, true).lines().map(str::to_owned).collect();
    let missing: Vec<&String> = tracked.difference(&covered).collect();
    let extra: Vec<&String> = covered.difference(&tracked).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "scripts/source-digest.sh and git disagree.\n  not covered: {missing:?}\n  \
         covered but not git's: {extra:?}\nAdd a new top-level entry to its list, or mirror a \
         new .gitignore rule in its exclusions."
    );
}

#[test]
fn the_release_records_this_digest() {
    let script = std::fs::read_to_string(repo_root().join("scripts/release-candidate.sh"))
        .expect("release script");
    assert!(
        script.contains("scripts/source-digest.sh"),
        "the release no longer records the build-input digest"
    );
}
