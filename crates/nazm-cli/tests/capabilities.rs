//! N37: authority is checked when a program is, and what runs is unchanged by it.
//!
//! * A capability parameter lowers exactly as an `Int` one: the same function body, word
//!   for word, whichever it is — so nothing at runtime consults it, and nothing could.
//! * The entry wrapper hands `main` one erased word per capability it declares, and a
//!   `main` without any is called as before.
//! * Every program in `examples/capabilities/`, one across two modules and one handing a
//!   capability to a task, gives the same output interpreted and compiled at `-O0` and
//!   `-O2`, and so do the migrated `examples/effects/`.

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

/// Every IR unit `nazm build --emit-ir` writes for the one-file program `text`, by name.
fn ir_units(text: &str) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), text).expect("write");
    let built = Command::new(NAZM)
        .args([
            "build",
            "m.nz",
            "-o",
            "prog",
            "--emit-ir",
            "--no-cache",
            "--opt-level",
            "0",
        ])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "building failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let mut units: Vec<(String, String)> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ll"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).expect("the IR"),
            )
        })
        .collect();
    units.sort();
    units
}

/// The definition of the Nazm function `name` — its symbol ends `.name` — from `define` to `}`.
fn definition(units: &[(String, String)], name: &str) -> String {
    for (_, ir) in units {
        if let Some(start) = ir
            .lines()
            .position(|l| l.starts_with("define") && l.contains(&format!(".{name}(")))
        {
            let lines: Vec<&str> = ir.lines().skip(start).collect();
            let end = lines.iter().position(|l| *l == "}").expect("the end");
            return lines[..=end].join("\n");
        }
    }
    panic!("no definition of {name}");
}

#[test]
fn a_capability_parameter_lowers_exactly_as_an_int_one_and_main_is_handed_its_roots() {
    // Signatures on lines of their own, so each body is at the same position in both.
    // Since N104 an `Int` cannot authorise `print`, so both bodies exercise nothing: what is
    // compared is how the parameter itself lowers.
    let cap = "fn say(io: IoCap, n: Int) -> Int\n{ n * 3 + 1 }\nfn main(io: IoCap, tasks: SpawnCap) -> Int\n{ say(io, 4); 0 }\n";
    let int = "fn say(io: Int, n: Int) -> Int\n{ n * 3 + 1 }\nfn main(io: IoCap, tasks: SpawnCap) -> Int\n{ say(0, 4); 0 }\n";
    let with_cap = ir_units(cap);
    let with_int = ir_units(int);
    assert_eq!(definition(&with_cap, "say"), definition(&with_int, "say"));
    // The entry hands `main` one word per capability, and `main` takes them as words.
    let entry = &with_cap
        .iter()
        .find(|(n, _)| n.contains("entry"))
        .expect("the entry unit")
        .1;
    // Two, not three: a `main` that writes an `IoCap` is given no `out` (R1, 5b.1).
    assert!(entry.contains("(i64 0, i64 0)"), "{entry}");
    assert!(!entry.contains("(i64 0, i64 0, i64 0)"), "{entry}");
    assert!(
        definition(&with_cap, "main").contains("(i64 %"),
        "{}",
        definition(&with_cap, "main")
    );
    // A `main` that writes no capability is given `out: OutCap` (R1, 5b.1), and handed that
    // one word and no other.
    let plain = ir_units("fn main() -> Int { 0 }\n");
    let entry = &plain.iter().find(|(n, _)| n.contains("entry")).unwrap().1;
    assert!(entry.contains(".main(i64 0)"), "{entry}");
    assert!(!entry.contains("i64 0, i64 0)"), "{entry}");
}

#[test]
fn a_main_that_takes_anything_but_authority_is_refused_before_it_is_built() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("m.nz"), "fn main(n: Int) -> Int { n }\n").unwrap();
    let built = Command::new(NAZM)
        .args(["build", "m.nz", "-o", "prog", "--no-cache"])
        .current_dir(dir.path())
        .output()
        .expect("build");
    assert!(!built.status.success());
    assert!(String::from_utf8_lossy(&built.stderr).contains("N0371"));
    assert!(!dir.path().join(crate::common::exe_name("prog")).exists());
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
fn every_capability_example_runs_the_same_interpreted_and_compiled() {
    let dir = tempfile::tempdir().expect("tempdir");
    copy_examples("examples/capabilities", dir.path());
    assert_eq!(agree(dir.path(), "workers.nz"), "1\n4\n9\n16\n30\n");
    assert_eq!(agree(dir.path(), "ledger.nz"), "books: 12\n");
}
