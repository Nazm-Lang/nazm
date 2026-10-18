//! Dead-stripping (R1.5, D14; `docs/architecture.md` §7.114): a hosted executable is linked with
//! `-dead_strip` (Mach-O) or `--gc-sections` over a section per function (ELF), so a standard
//! function the program never calls is not in it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Calls one function of `@std/text`, and none of the others.
const PROGRAM: &str =
    "use \"@std/text\"\n\nfn main() {\n    println(text_starts_with(\"nazm\", \"na\"))\n}\n";

fn nazm(dir: &Path, args: &[&str]) {
    let out = Command::new(NAZM)
        .args(args)
        .env("NAZM_USER_CACHE_DIR", "")
        .current_dir(dir)
        .output()
        .expect("nazm");
    assert!(
        out.status.success(),
        "nazm {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The symbols an executable defines, as `nm` prints them.
fn symbols(exe: &Path) -> String {
    let out = Command::new("nm").arg(exe).output().expect("nm");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn an_unused_standard_function_is_left_out_and_the_program_still_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.nz"), PROGRAM).expect("program");
    for backend in ["llvm", "cranelift"] {
        for opt in ["0", "2"] {
            let exe = crate::common::exe_name(format!("prog-{backend}-{opt}"));
            nazm(
                dir.path(),
                &[
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "--opt-level",
                    opt,
                    "main.nz",
                    "-o",
                    &exe,
                ],
            );
            let exe = dir.path().join(crate::common::exe_name(&exe));
            let out = Command::new(&exe).output().expect("run");
            assert!(out.status.success(), "{backend} -O{opt}");
            assert!(
                String::from_utf8_lossy(&out.stdout).starts_with("true\n"),
                "{backend} -O{opt}"
            );
            let defined = symbols(&exe);
            // `@std/text`'s `text_repeat`, which nothing calls, by its symbol's spelling.
            assert!(
                !defined.contains("text_5Frepeat"),
                "{backend} -O{opt}: an unused standard function was linked in"
            );
            if opt == "0" && backend == "llvm" {
                assert!(
                    defined.contains("text_5Fstarts_5Fwith"),
                    "the function the program calls is gone"
                );
            }
        }
    }
}

#[test]
fn a_dead_stripped_executable_is_smaller_than_the_same_objects_linked_whole() {
    // The same objects, from `--objects`, linked as `link.txt` says without stripping: the build's
    // own executable is smaller, and only the whole link holds the unused function.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.nz"), PROGRAM).expect("program");
    nazm(
        dir.path(),
        &["build", "--no-cache", "main.nz", "-o", "stripped"],
    );
    nazm(
        dir.path(),
        &["build", "--no-cache", "main.nz", "--objects", "objects"],
    );
    let listing =
        std::fs::read_to_string(dir.path().join("objects").join("link.txt")).expect("link.txt");
    let field = |name: &str| {
        listing
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name} ")))
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let whole = dir.path().join("whole");
    let out = Command::new("clang")
        .arg("-o")
        .arg(&whole)
        .args(
            field("objects")
                .iter()
                .map(|o| dir.path().join("objects").join(o)),
        )
        .args(field("libraries"))
        .output()
        .expect("clang");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(Command::new(&whole).output().expect("run").status.success());

    let size = |p: &Path| std::fs::metadata(p).expect("size").len();
    let stripped = dir.path().join(crate::common::exe_name("stripped"));
    assert!(
        size(&stripped) < size(&whole),
        "dead-stripping saved nothing: {} against {} bytes",
        size(&stripped),
        size(&whole)
    );
    assert!(symbols(&whole).contains("text_5Frepeat"));
    assert!(!symbols(&stripped).contains("text_5Frepeat"));
}
