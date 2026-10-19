//! R1e, S1: `file_walk` in `@std/file` (`docs/user/spec.md`, *The standard library*).
//!
//! A tree with nested and empty directories, names whose byte order differs from a depth-first
//! listing's (`a.txt` sorts before `a/x`) and a capital letter,
//! walked under the interpreter and LLVM: every path, sorted by bytes, never the root.
//! On Unix, what symbolic links do, as the spec says since R1d (`os_lstat`): a link is listed and
//! never followed — not one to a directory, not a dangling one, not one that reaches its own
//! ancestor. A missing directory and a file are each an `Err` of their kind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const WALKER: &str = r#"use "@std/file";
use "@std/ioerror";

fn show(io: IoCap, dir: Str) -> Int ! {io} {
    match file_walk(io, dir) {
        Result.Ok(value: paths) => {
            print(f"{dir}: {strs_len(paths)}\n");
            for p in paths { print(f"  {p}\n"); }
            0
        },
        Result.Err(error: e) => { print(f"{dir}: {io_kind_name(e.kind)}\n"); 0 },
    }
}

fn main(io: IoCap) -> Int ! {io} {
    show(io, "tree");
    show(io, "tree/");
    show(io, "tree/empty");
    show(io, "missing");
    show(io, "tree/a.txt");
    0
}
"#;

const EXPECTED: &str = "tree: 9
  tree/Z
  tree/a
  tree/a.txt
  tree/a/x
  tree/b
  tree/b/c
  tree/b/c/deep
  tree/b/z
  tree/empty
tree/: 9
  tree/Z
  tree/a
  tree/a.txt
  tree/a/x
  tree/b
  tree/b/c
  tree/b/c/deep
  tree/b/z
  tree/empty
tree/empty: 0
missing: NotFound
tree/a.txt: NotADirectory
0
";

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn make_tree(root: &Path) {
    let t = root.join("tree");
    for d in ["a", "b/c", "empty"] {
        std::fs::create_dir_all(t.join(d)).unwrap();
    }
    for f in ["a.txt", "a/x", "b/c/deep", "b/z", "Z"] {
        std::fs::write(t.join(f), f).unwrap();
    }
}

/// The walker's output under the interpreter and under LLVM.
fn walk_everywhere(dir: &Path, program: &str) -> Vec<(String, Ran)> {
    std::fs::write(dir.join("prog.nz"), program).unwrap();
    let mut runs = vec![(
        "interpreter".to_owned(),
        nazm(dir, &["run", "--interpret", "prog.nz"]),
    )];
    let exe = dir.join(crate::common::exe_name("walker"));
    let b = nazm(
        dir,
        &[
            "build",
            "--no-cache",
            "--backend",
            "llvm",
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ],
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    runs.push((
        "llvm".to_owned(),
        run_bounded(common::bounded_exe(&exe).current_dir(dir), "llvm"),
    ));
    runs
}

#[test]
fn a_walk_lists_every_file_and_directory_sorted() {
    let d = tempfile::tempdir().unwrap();
    make_tree(d.path());
    for (label, r) in walk_everywhere(d.path(), WALKER) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, EXPECTED, "{label}");
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_listed_and_never_followed() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    make_tree(d.path());
    let t = d.path().join("tree");
    symlink("../a.txt", t.join("b/link-file")).unwrap();
    symlink("../a", t.join("b/link-dir")).unwrap();
    symlink("nowhere", t.join("dangling")).unwrap();
    std::fs::create_dir(d.path().join("loop")).unwrap();
    symlink("..", d.path().join("loop/up")).unwrap();
    let program = WALKER.replace(
        "    show(io, \"tree/\");\n    show(io, \"tree/empty\");\n    show(io, \"missing\");\n    show(io, \"tree/a.txt\");\n",
        "    show(io, \"loop\");\n",
    );
    let want = "tree: 12
  tree/Z
  tree/a
  tree/a.txt
  tree/a/x
  tree/b
  tree/b/c
  tree/b/c/deep
  tree/b/link-dir
  tree/b/link-file
  tree/b/z
  tree/dangling
  tree/empty
loop: 1
  loop/up
0
";
    for (label, r) in walk_everywhere(d.path(), &program) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, want, "{label}");
    }
}
