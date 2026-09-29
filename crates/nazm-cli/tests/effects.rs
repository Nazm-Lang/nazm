//! N36: effects are a fact for the checker, and leave no trace in what runs.
//!
//! * A program with its effects declared compiles to exactly the IR of the same program
//!   without them: no runtime tag, no dispatch, no object — at `-O0`, the level at which a
//!   difference could not be optimised away.
//! * Every program in `examples/effects/`, one of them across two modules, is accepted and
//!   gives the same output interpreted and compiled at `-O0` and `-O2`, spawning included.

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
    let mut child = Command::new(dir.join("prog"))
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

/// Every IR unit `nazm build --emit-ir` writes for `files` (the first is the root), in
/// name order.
fn ir_of(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).expect("write");
    }
    let built = Command::new(NAZM)
        .args([
            "build",
            files[0].0,
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
    let mut units: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ll"))
        .collect();
    units.sort();
    assert!(!units.is_empty(), "no IR was emitted");
    units
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("the IR"))
        .collect()
}

// Each signature on a line of its own, so every body is at the same line and column in
// both: a runtime error message carries its position, and an annotation on the body's line
// would move it. Both hold the same capabilities (N37), so the effect sets are the only
// difference; what capabilities cost is `tests/capabilities.rs`'s.
const PLAIN: &str = "fn worker(c: Chan) -> Int\n{ chan_send(c, 7); 0 }\nfn say(io: IoCap, n: Int) -> Int\n{ print(int_to_str(n)) }\nfn sum(n: Int) -> Int\n{ let mut t = 0; let mut i = 1; while i <= n { t = t + i; i = i + 1; } t }\nfn run(io: IoCap, tasks: SpawnCap) -> Int\n{ let c = chan_new(1); let buf = ints_new(); scope { spawn worker(c); chan_recv(c, buf); } say(io, ints_pop(buf) + sum(3)) }\nfn main(io: IoCap, tasks: SpawnCap) -> Int\n{ run(io, tasks); 0 }\n";

const TYPED: &str = "fn worker(c: Chan) -> Int ! {}\n{ chan_send(c, 7); 0 }\nfn say(io: IoCap, n: Int) -> Int ! { io }\n{ print(int_to_str(n)) }\nfn sum(n: Int) -> Int ! {}\n{ let mut t = 0; let mut i = 1; while i <= n { t = t + i; i = i + 1; } t }\nfn run(io: IoCap, tasks: SpawnCap) -> Int ! { spawn, io }\n{ let c = chan_new(1); let buf = ints_new(); scope { spawn worker(c); chan_recv(c, buf); } say(io, ints_pop(buf) + sum(3)) }\nfn main(io: IoCap, tasks: SpawnCap) -> Int ! { io, spawn }\n{ run(io, tasks); 0 }\n";

#[test]
fn a_declared_effect_set_leaves_no_trace_in_the_native_program() {
    assert_eq!(ir_of(&[("m.nz", PLAIN)]), ir_of(&[("m.nz", TYPED)]));
    // Across modules too: a contract in an interface is not in the object either.
    let body = "{ let mut t = 0; let mut i = 1; while i <= n { t = t + i; i = i + 1; } t }";
    let lib_plain = format!("pub fn total(n: Int) -> Int\n{body}\n");
    let lib_typed = format!("pub fn total(n: Int) -> Int ! {{}}\n{body}\n");
    let main_plain = "use \"lib.nz\";\nfn main() -> Int\n{ total(4) }\n";
    let main_typed = "use \"lib.nz\";\nfn main() -> Int ! {}\n{ total(4) }\n";
    assert_eq!(
        ir_of(&[("main.nz", main_plain), ("lib.nz", &lib_plain)]),
        ir_of(&[("main.nz", main_typed), ("lib.nz", &lib_typed)])
    );
}

/// Interpret `root` in `dir`, then build and run it at `-O0` and `-O2`: all three agree.
fn agree(dir: &Path, root: &str) -> String {
    let interpreted = Command::new(NAZM)
        .args(["run", root])
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
    // The program's own output, then `main`'s value on a line of its own — from both.
    stdout
        .strip_suffix("0\n")
        .unwrap_or_else(|| panic!("{root}: {stdout:?}"))
        .to_owned()
}

#[test]
fn every_effects_example_runs_the_same_interpreted_and_compiled() {
    let src = repo_root().join("examples/effects");
    let dir = tempfile::tempdir().expect("tempdir");
    for e in std::fs::read_dir(&src).expect("examples/effects").flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "nz") {
            std::fs::copy(&p, dir.path().join(e.file_name())).expect("copy");
        }
    }
    assert_eq!(agree(dir.path(), "pure.nz"), "1275\n");
    assert_eq!(agree(dir.path(), "tasks.nz"), "30\n");
    assert_eq!(agree(dir.path(), "report.nz"), "10\n");
    // And the typed program above, which spawns through a declared helper.
    std::fs::write(dir.path().join("typed.nz"), TYPED).expect("write");
    assert_eq!(agree(dir.path(), "typed.nz"), "13");
}
