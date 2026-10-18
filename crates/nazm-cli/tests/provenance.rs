//! N38: provenance is a checker fact and leaves nothing in what runs.
//!
//! * Every program in `examples/provenance/` — two modules, a file read, pure computation over
//!   it and a write to a path the program chose — gives the same output interpreted and
//!   compiled at `-O0` and `-O2`.
//! * A program refused for a restricted flow is refused before anything is built.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the repository root")
        .to_path_buf()
}

/// Run the compiled `prog` in `dir` under a deadline: `(status, stdout)`.
fn run_native(dir: &Path) -> (Option<i32>, String) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(60);
    let out_path = dir.join("prog.out");
    let mut child = Command::new(dir.join(crate::common::exe_name("prog")))
        .current_dir(dir)
        .stdout(std::fs::File::create(&out_path).expect("stdout file"))
        .spawn()
        .expect("execute");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the compiled program did not finish within {LIMIT:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    (
        status.code(),
        std::fs::read_to_string(&out_path).unwrap_or_default(),
    )
}

/// Interpret `root` in `dir`, then build and run it at `-O0` and `-O2`: all three agree.
fn agree(dir: &Path, root: &str) -> String {
    let interpreted = Command::new(NAZM)
        .args(["run", "--interpret", root])
        .current_dir(dir)
        .output()
        .expect("run");
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "interpreting {root}: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    let stdout = String::from_utf8_lossy(&interpreted.stdout).into_owned();
    for level in ["0", "2"] {
        let built = Command::new(NAZM)
            .args([
                "build",
                root,
                "-o",
                "prog",
                "--no-cache",
                "--opt-level",
                level,
            ])
            .current_dir(dir)
            .output()
            .expect("build");
        assert!(
            built.status.success(),
            "building {root} at -O{level}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let (status, native) = run_native(dir);
        assert_eq!(status, Some(0), "{root} at -O{level}");
        assert_eq!(native, stdout, "{root} at -O{level}");
    }
    stdout
        .strip_suffix("0\n")
        .unwrap_or_else(|| panic!("{root}: {stdout:?}"))
        .to_owned()
}

fn copy_examples(from: &str, to: &Path) {
    let src = repo_root().join(from);
    for e in std::fs::read_dir(&src).expect("examples").flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "nz") {
            std::fs::copy(&p, to.join(e.file_name())).expect("copy");
        }
    }
}

#[test]
fn every_provenance_example_runs_the_same_interpreted_and_compiled() {
    let dir = tempfile::tempdir().expect("tempdir");
    copy_examples("examples/provenance", dir.path());
    let text = std::fs::read_to_string(dir.path().join("tally.nz")).expect("tally.nz");
    let lines = text.matches('\n').count();
    assert_eq!(agree(dir.path(), "tally.nz"), format!("{lines}\n"));
    // And it wrote where it chose to.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("tally.out")).expect("tally.out"),
        lines.to_string()
    );
}

#[test]
fn a_restricted_flow_is_refused_before_anything_is_built() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("m.nz"),
        "fn main(io: IoCap) -> Int ! { io } { write_file(read_file(\"m.nz\"), \"x\"); 0 }\n",
    )
    .unwrap();
    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog", "--no-cache"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(!built.status.success());
    assert!(String::from_utf8_lossy(&built.stderr).contains("N0372"));
    assert!(!dir.path().join(crate::common::exe_name("prog")).exists());
}

/// Q1-D-19: a file's contents carried through Gate 2's `Bytes` and `Text` — made from, sliced,
/// turned back into a `Str` — keep their provenance: as a write's path each is refused with
/// `N0372`, like the `Str` itself; as the data written, each is accepted.
#[test]
fn provenance_survives_bytes_and_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let check = |body: &str| {
        std::fs::write(
            dir.path().join("p.nz"),
            format!("fn main(io: IoCap) -> Int ! {{ io }} {{ {body} }}\n"),
        )
        .unwrap();
        let o = Command::new(NAZM)
            .args(["check", "p.nz"])
            .current_dir(dir.path())
            .output()
            .expect("check");
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    };
    for path in [
        "bytes_to_str(bytes_from_str(read_file(\"cfg\")))",
        "bytes_to_str(bytes_slice(bytes_from_str(read_file(\"cfg\")), 0, 2))",
        "text_as_str(Text(read_file(\"cfg\")))",
        "text_as_str(text_slice(Text(read_file(\"cfg\")), 0, 1))",
    ] {
        let (code, err) = check(&format!("write_file({path}, \"x\")"));
        assert_eq!(code, Some(1), "{path}: {err}");
        assert!(err.contains("N0372"), "{path}: {err}");
        let (code, err) = check(&format!("write_file(arg(0), {path})"));
        assert_eq!(code, Some(0), "{path} as data: {err}");
    }
}
