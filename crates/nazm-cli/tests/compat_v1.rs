//! The Nazm 1.0 compatibility corpus, `tests/compat/v1/` (Gate 1).
//!
//! `docs/stability.md` promises that a program valid under stable Nazm 1.0, using only stable
//! public surfaces, does not silently change meaning under a later 1.x toolchain. This is what
//! holds a later toolchain to that: representative stable programs with what they print,
//! programs that must trap with the code they trap with, programs that must be refused with the
//! code they are refused with, and programs that must check.
//!
//! The corpus is append-only within 1.x. Every file is pinned by its BLAKE3 digest in
//! `BLAKE3SUMS`, so editing a case is a visible change to the pins, not a quiet one — and an
//! edited expectation is a changed meaning, which is a 2.0 change or a recorded defect fix
//! (`tests/compat/v1/README.md`).
//!
//! | directory | a case is | held to |
//! | --- | --- | --- |
//! | `run/` | `NAME.nz`, or a directory `NAME/` with `main.nz` or a `nazm.toml` | `NAME.stdout`, exactly, from `nazm run` and from executables built by both backends |
//! | `trap/` | `NAME.nz`, first line `// expect: CODE` | exit status 2 and `CODE`, interpreted and compiled by both backends |
//! | `refuse/` | `NAME.nz`, first line `// expect: CODE` | `nazm check` exits 1 and its first error is `CODE`; `nazm build` refuses too |
//! | `check/` | `NAME.nz` | `nazm check` exits 0 |
//!
//! A refusal case whose construct a later 1.x added — `docs/stability.md` makes adding a
//! construct 1.0 refused by name a minor addition, because no valid program changes meaning —
//! is listed in `SUPERSEDED` with the addition that superseded it. It stays in the corpus,
//! unedited and pinned, and is held to the opposite: it must now check.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Fewer cases than this in a directory means the corpus lost some, not that it is small.
const AT_LEAST: [(&str, usize); 4] = [("run", 22), ("trap", 7), ("refuse", 36), ("check", 1)];

/// The corpus as committed. Only read: a run writes caches beside what it checks, so every
/// case is run from a [`staged`] copy and the tree is never written to.
fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/compat/v1")
        .canonicalize()
        .expect("tests/compat/v1")
}

/// A copy of the corpus in a scratch directory, for the cases to run in.
fn staged() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = committed();
    let mut files = Vec::new();
    every_file(&root, &mut files);
    for f in files {
        let to = dir.path().join(f.strip_prefix(&root).unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(&f, &to).unwrap();
    }
    dir
}

fn nazm(args: &[&str], cwd: &Path) -> Output {
    Command::new(NAZM)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run nazm")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The cases of one directory, by name: the path `nazm` is given for each.
fn cases(corpus: &Path, kind: &str) -> BTreeMap<String, PathBuf> {
    let dir = corpus.join(kind);
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.unwrap().path();
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            let entry = if p.join("nazm.toml").is_file() {
                p.clone()
            } else {
                p.join("main.nz")
            };
            assert!(entry.exists(), "{}: no main.nz or nazm.toml", p.display());
            out.insert(name, entry);
        } else if p.extension().is_some_and(|x| x == "nz") {
            out.insert(name, p);
        }
    }
    out
}

/// The code a case's first line says it expects: `// expect: N0400`.
fn expected_code(src: &Path) -> String {
    let first = std::fs::read_to_string(src).unwrap();
    let first = first.lines().next().unwrap_or_default();
    first
        .strip_prefix("// expect: ")
        .unwrap_or_else(|| panic!("{}: first line is not `// expect: CODE`", src.display()))
        .trim()
        .to_owned()
}

fn every_file(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            every_file(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn every_corpus_file_is_pinned_by_its_digest() {
    let root = committed();
    let sums = std::fs::read_to_string(root.join("BLAKE3SUMS")).expect("BLAKE3SUMS");
    let mut pinned = BTreeMap::new();
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (digest, path) = line.split_once("  ").expect("`DIGEST  PATH`");
        pinned.insert(path.to_owned(), digest.to_owned());
    }
    let mut files = Vec::new();
    every_file(&root, &mut files);
    let mut seen = BTreeMap::new();
    for f in files {
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "BLAKE3SUMS" || rel == "README.md" {
            continue;
        }
        let digest = blake3::hash(&std::fs::read(&f).unwrap())
            .to_hex()
            .to_string();
        seen.insert(rel, digest);
    }
    for (path, digest) in &seen {
        match pinned.get(path) {
            None => panic!("{path} is not pinned in BLAKE3SUMS: a new case is added with its pin"),
            Some(d) => assert_eq!(
                d, digest,
                "{path} changed. A 1.x toolchain keeps every case's meaning: an edited case or \
                 expectation is a 2.0 change or a recorded defect fix (README.md)"
            ),
        }
    }
    for path in pinned.keys() {
        assert!(
            seen.contains_key(path),
            "{path} is pinned but gone: cases are never removed in 1.x"
        );
    }
    for (kind, n) in AT_LEAST {
        assert!(
            cases(&root, kind).len() >= n,
            "{kind}/ holds fewer than {n} cases"
        );
    }
}

#[test]
fn every_run_case_prints_what_it_says_interpreted_and_compiled_by_both_backends() {
    let scratch = tempfile::tempdir().unwrap();
    let corpus = staged();
    for (name, entry) in cases(corpus.path(), "run") {
        let cwd = if entry.is_dir() {
            entry.clone()
        } else {
            entry.parent().unwrap().to_path_buf()
        };
        let expected =
            std::fs::read_to_string(corpus.path().join("run").join(format!("{name}.stdout")))
                .unwrap_or_else(|_| panic!("run/{name}.stdout"));
        let arg = entry.to_str().unwrap();

        let out = nazm(&["check", arg], &cwd);
        assert_eq!(
            out.status.code(),
            Some(0),
            "check {name}: {}",
            text(&out.stderr)
        );

        let out = nazm(&["run", arg], &cwd);
        assert_eq!(
            out.status.code(),
            Some(0),
            "run {name}: {}",
            text(&out.stderr)
        );
        assert_eq!(text(&out.stdout), expected, "run {name}");

        for backend in ["llvm", "cranelift"] {
            let exe = scratch.path().join(format!("{name}.{backend}"));
            let out = nazm(
                &[
                    "build",
                    arg,
                    "-o",
                    exe.to_str().unwrap(),
                    "--backend",
                    backend,
                ],
                &cwd,
            );
            assert_eq!(
                out.status.code(),
                Some(0),
                "build {name} ({backend}): {}",
                text(&out.stderr)
            );
            let ran = Command::new(&exe).current_dir(&cwd).output().unwrap();
            assert_eq!(
                ran.status.code(),
                Some(0),
                "{name} ({backend}): {}",
                text(&ran.stderr)
            );
            assert_eq!(text(&ran.stdout), expected, "{name} built by {backend}");
        }
    }
}

#[test]
fn every_trap_case_traps_with_its_code_interpreted_and_compiled_by_both_backends() {
    let scratch = tempfile::tempdir().unwrap();
    let corpus = staged();
    for (name, entry) in cases(corpus.path(), "trap") {
        let cwd = entry.parent().unwrap();
        let code = expected_code(&entry);
        let arg = entry.to_str().unwrap();

        let out = nazm(&["check", arg], cwd);
        assert_eq!(
            out.status.code(),
            Some(0),
            "check {name}: {}",
            text(&out.stderr)
        );

        let out = nazm(&["run", arg], cwd);
        assert_eq!(out.status.code(), Some(2), "run {name}: {out:?}");
        assert!(
            text(&out.stderr).contains(&format!("error[{code}]")),
            "run {name}: {}",
            text(&out.stderr)
        );

        for backend in ["llvm", "cranelift"] {
            let exe = scratch.path().join(format!("{name}.{backend}"));
            let out = nazm(
                &[
                    "build",
                    arg,
                    "-o",
                    exe.to_str().unwrap(),
                    "--backend",
                    backend,
                ],
                cwd,
            );
            assert_eq!(
                out.status.code(),
                Some(0),
                "build {name} ({backend}): {}",
                text(&out.stderr)
            );
            let ran = Command::new(&exe).current_dir(cwd).output().unwrap();
            assert_eq!(ran.status.code(), Some(2), "{name} ({backend}): {ran:?}");
            assert!(
                text(&ran.stderr).contains(&format!("[{code}]")),
                "{name} ({backend}): {}",
                text(&ran.stderr)
            );
        }
    }
}

/// The refusal cases a later 1.x superseded by adding their construct, by path from the corpus
/// root, each with the addition that superseded it (`SUPERSEDED`, pinned like every case).
fn superseded(root: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(root.join("SUPERSEDED")).unwrap_or_default();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (path, why) = l.split_once("  ").expect("`PATH  WHY`");
            (path.to_owned(), why.to_owned())
        })
        .collect()
}

#[test]
fn a_superseded_refusal_is_kept_and_now_checks() {
    let corpus = staged();
    let listed = superseded(&committed());
    assert!(
        !listed.is_empty(),
        "SUPERSEDED lists the cases a 1.x addition superseded"
    );
    for (path, why) in &listed {
        assert!(!why.trim().is_empty(), "{path}: SUPERSEDED says why");
        let entry = corpus.path().join(path);
        assert!(entry.is_file(), "{path} is superseded, never removed");
        let out = nazm(&["check", entry.to_str().unwrap()], entry.parent().unwrap());
        assert_eq!(
            out.status.code(),
            Some(0),
            "{path} is superseded by an addition, so it checks now: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn every_refusal_case_is_refused_with_its_code_and_not_built() {
    let scratch = tempfile::tempdir().unwrap();
    let corpus = staged();
    let superseded = superseded(&committed());
    for (name, entry) in cases(corpus.path(), "refuse") {
        if superseded.contains_key(&format!("refuse/{name}.nz")) {
            continue;
        }
        let cwd = entry.parent().unwrap();
        let code = expected_code(&entry);
        let arg = entry.to_str().unwrap();

        let out = nazm(&["check", "--json", arg], cwd);
        assert_eq!(out.status.code(), Some(1), "check {name}: {out:?}");
        // Diagnostics are written to standard error, one `nazm.diagnostic/1` object a line.
        let first = text(&out.stderr)
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|d| d["severity"] == "error")
            .unwrap_or_else(|| panic!("{name}: no error reported: {out:?}"));
        assert_eq!(first["code"], code.as_str(), "{name}: {first}");

        let exe = scratch.path().join(&name);
        let out = nazm(&["build", arg, "-o", exe.to_str().unwrap()], cwd);
        assert_eq!(out.status.code(), Some(1), "build {name}: {out:?}");
        assert!(!exe.exists(), "{name}: a refused program was built");
    }
}

#[test]
fn every_check_case_checks() {
    let corpus = staged();
    for (name, entry) in cases(corpus.path(), "check") {
        let cwd = entry.parent().unwrap();
        let out = nazm(&["check", entry.to_str().unwrap()], cwd);
        assert_eq!(
            out.status.code(),
            Some(0),
            "check {name}: {}",
            text(&out.stderr)
        );
    }
}
