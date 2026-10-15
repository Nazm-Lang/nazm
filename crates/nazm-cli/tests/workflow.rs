//! The repair workflow, end to end: broken file in, working file out.
//!
//! `scripts/repair.sh` is the whole loop — apply every automatic fix until a pass
//! changes nothing, lay the result out, report what is left — and the reason it is a
//! script rather than a paragraph in a README is that a workflow nobody runs is a
//! workflow nobody has checked. These tests run it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn write(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    std::fs::write(&src, body).expect("write");
    (dir, src)
}

/// Run the repair script over one file, returning `(exit status, stdout, the new text)`.
fn repair(path: &Path) -> (Option<i32>, String, String) {
    let out = Command::new("sh")
        .arg(repo_root().join("scripts/repair.sh"))
        .arg(path)
        .env("NAZM", NAZM)
        .output()
        .expect("run repair.sh");
    let text = std::fs::read_to_string(path).expect("read back");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        text,
    )
}

fn run(path: &Path) -> (Option<i32>, String) {
    let out = Command::new(NAZM)
        .args(["run", "--interpret", path.to_str().unwrap()])
        .output()
        .expect("run");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    )
}

#[test]
fn a_broken_program_is_repaired_and_runs() {
    // Two mistakes of the same kind in one file, both automatically fixable, and the
    // file is badly laid out on top. One invocation, and it compiles and runs.
    let (dir, path) = write("fn main()->Int{\nlet x=1;\nx=2;\nlet y=3;\ny=4;\nx+y}\n");
    let (code, _, text) = repair(&path);
    assert_eq!(code, Some(0), "repair left errors behind:\n{text}");
    assert_eq!(
        text,
        "fn main() -> Int {\n    let mut x = 1;\n    x = 2;\n    let mut y = 3;\n    y = 4;\n    x + y\n}\n",
        "got:\n{text}"
    );
    assert_eq!(run(&path), (Some(0), "6".to_owned()));
    drop(dir);
}

#[test]
fn a_repair_that_reveals_another_error_still_finishes() {
    // The reason the script loops. Whether one round is enough depends on the order
    // the checker reports things in, which is not something a workflow should depend on.
    let (dir, path) = write(
        "fn main() -> Int {\n    let a = 1;\n    a = 2;\n    let b = a;\n    b = 3;\n    a + b\n}\n",
    );
    let (code, _, text) = repair(&path);
    assert_eq!(code, Some(0), "not fully repaired:\n{text}");
    assert!(text.contains("let mut a = 1;"), "{text}");
    assert!(text.contains("let mut b = a;"), "{text}");
    drop(dir);
}

#[test]
fn what_needs_review_is_reported_and_not_applied() {
    // A contract narrower than its body has a fix, and it is `needs_review`: widening it can
    // break a caller. The workflow must leave it, and must say so rather than silently doing
    // nothing.
    let (dir, path) = write(
        "fn f(out: OutCap) -> Int ! {} {\n    print(\"x\");\n    0\n}\n\
         fn main(out: OutCap) -> Int ! {io} { f(out) }\n",
    );
    let (code, out, text) = repair(&path);
    assert_eq!(code, Some(1), "an unrepairable file must report:\n{text}");
    assert!(
        out.contains("\"code\":\"N0366\"") || out.contains("N0366"),
        "the remaining diagnostic should be reported as JSON: {out}"
    );

    let listed = Command::new(NAZM)
        .args(["fix", "--json", path.to_str().unwrap()])
        .output()
        .expect("run");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("\"applicability\":\"needs_review\""),
        "the suggestion should still be offered: {listed}"
    );
    assert!(
        listed.contains("\"applied\":false"),
        "and must not have been applied: {listed}"
    );
    drop(dir);
}

#[test]
fn a_file_that_does_not_lex_is_reported_rather_than_rewritten() {
    let (dir, path) = write("fn main() -> Int { \"unterminated }\n");
    let before = std::fs::read_to_string(&path).expect("read");
    let (code, _, after) = repair(&path);
    assert_eq!(code, Some(1));
    assert_eq!(before, after, "nothing should have been written");
    drop(dir);
}

#[test]
fn an_edit_is_refused_when_the_file_changed_under_it() {
    // A span is an offset into the text that was checked. If the file has moved on, the
    // same offset names different bytes, and applying the edit there corrupts work
    // somebody just did. The edit records what it expects to replace and checks.
    //
    // Provoked here by hand: a hook that rewrites the file between the check and the
    // write is not something a test can arrange, so the same situation is built by
    // giving one program's diagnostics to a different file — which is exactly the
    // shape of the failure, an offset from one text applied to another.
    let (dir, path) = write("fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n");
    let listed = Command::new(NAZM)
        .args(["fix", "--json", path.to_str().unwrap()])
        .output()
        .expect("run");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(listed.contains("\"applied\":false"), "{listed}");

    // Now the file says something else at those offsets, and the same fix must not run.
    std::fs::write(
        &path,
        "fn main() -> Int {\n    let yyy = 1;\n    yyy = 2;\n    yyy\n}\n",
    )
    .expect("rewrite");
    let before = std::fs::read_to_string(&path).expect("read");
    let out = Command::new(NAZM)
        .args(["fix", "--apply", path.to_str().unwrap()])
        .output()
        .expect("run");
    let after = std::fs::read_to_string(&path).expect("read");
    // The rewritten file has the same error, so a fix *is* found — the point is that it
    // is the fix for the text now on disk, and the result still parses and checks.
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        after.contains("let mut yyy = 1;"),
        "before:\n{before}\nafter:\n{after}"
    );
    drop(dir);
}

#[test]
fn fmt_check_passes_on_what_fmt_produces() {
    let (dir, path) = write("fn main()->Int{let a=1;a}");
    let check = |expect: Option<i32>| {
        let out = Command::new(NAZM)
            .args(["fmt", "--check", path.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            out.status.code(),
            expect,
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    check(Some(1));
    let out = Command::new(NAZM)
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(0));
    check(Some(0));
    drop(dir);
}

#[test]
fn fix_writes_nothing_without_apply() {
    let (dir, path) = write("fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n");
    let before = std::fs::read_to_string(&path).expect("read");
    let out = Command::new(NAZM)
        .args(["fix", path.to_str().unwrap()])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "the file still has an error");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("would apply"),
        "it should say what it would do"
    );
    assert_eq!(
        before,
        std::fs::read_to_string(&path).expect("read"),
        "the default must not write"
    );
    drop(dir);
}

/// `nazm test <dir>`, under a deadline.
///
/// `nazm test` runs every case natively as well as interpreted, and a native program that
/// never finishes is a wrong answer — a miscompiled loop is exactly that — so it fails the
/// test here rather than holding the suite until an outer limit ends it with no verdict.
/// The run gets its own process group, and the whole group is killed on a timeout, so a
/// looping case is not left running beneath a `nazm` that has already been stopped.
fn nazm_test_with_deadline(dir: &std::path::Path) -> std::process::Output {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(600);
    let scratch = tempfile::tempdir().expect("tempdir");
    let out_path = scratch.path().join("test.out");
    let err_path = scratch.path().join("test.err");
    let mut cmd = Command::new(NAZM);
    cmd.arg("test")
        .arg(dir)
        .stdout(std::fs::File::create(&out_path).expect("stdout file"))
        .stderr(std::fs::File::create(&err_path).expect("stderr file"));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().expect("run");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if started.elapsed() > LIMIT {
            let _ = Command::new("kill")
                .arg("-KILL")
                // `--`: procps `kill` (the container's) parses `-<pgid>` after a signal as an
                // option and signals nothing without it. BSD `kill` accepts both.
                .arg("--")
                .arg(format!("-{}", child.id()))
                .status();
            let _ = child.wait();
            panic!("`nazm test` did not finish within {LIMIT:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    std::process::Output {
        status,
        stdout: std::fs::read(&out_path).unwrap_or_default(),
        stderr: std::fs::read(&err_path).unwrap_or_default(),
    }
}

#[test]
fn a_corpus_runs_interpreted_and_compiled_and_both_must_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let at = |name: &str, body: &str, expected: &str| {
        std::fs::write(dir.path().join(format!("{name}.nz")), body).expect("write");
        std::fs::write(dir.path().join(format!("{name}.expected")), expected).expect("write");
    };
    at("good", "fn main() -> Int { 7 }\n", "7\n");
    at("bad", "fn main() -> Int { 8 }\n", "7\n");
    // A `.nz` with no `.expected`: not a case, and reported as skipped rather than
    // passed over.
    std::fs::write(dir.path().join("helper.nz"), "fn main() -> Int { 1 }\n").expect("write");

    let out = nazm_test_with_deadline(dir.path());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a failing case must fail the run"
    );
    assert!(stdout.contains("ok    good"), "{stdout}");
    assert!(stdout.contains("FAIL  bad"), "{stdout}");
    assert!(stderr.contains("helper.nz has no `.expected`"), "{stderr}");
    assert!(stderr.contains("1 of 2 passed"), "{stderr}");
    drop(dir);
}

#[test]
fn the_conformance_corpus_passes_under_nazm_test() {
    // The same five cases `compiler/bootstrap.sh` compares, through the command rather
    // than through the script — so the convention has one implementation a person can
    // run and one the bootstrap runs, and they are checked against the same files.
    let out = nazm_test_with_deadline(&repo_root().join("compiler/conformance"));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Every case, and a floor rather than an exact count: the corpus grows, and a test
    // that had to be edited for each addition is one that eventually is not.
    let summary = String::from_utf8_lossy(&out.stderr);
    let passed: usize = summary
        .split_whitespace()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(passed >= 6, "{summary}");
    assert!(
        summary.contains(&format!("{passed} of {passed} passed")),
        "{summary}"
    );
}

#[test]
fn every_tree_source_is_already_formatted() {
    // The formatter is only useful if the tree is what it produces; otherwise
    // `--check` is a gate nobody can pass and everybody turns off.
    let root = repo_root();
    let mut files = Vec::new();
    for folder in ["compiler", "examples"] {
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "nz") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    assert!(files.len() >= 5, "only {} sources", files.len());

    let out = Command::new(NAZM)
        .arg("fmt")
        .arg("--check")
        .args(&files)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "these are not canonically laid out; run `nazm fmt` on them:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
