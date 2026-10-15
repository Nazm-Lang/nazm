//! The differential harness: one program under the interpreter and built by each backend at each
//! optimisation level, compared on stdout, exit status and memory report; and the codes `nazm
//! check --json` reports for a program it refuses. On macOS the backends include Nazm's own code
//! generator (R9, D15: `--backend nazm`), built once per test process by [`nzcg`].

// Each test binary that includes this module uses only part of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::common::{self, run_bounded, Ran};

pub(crate) const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

pub(crate) fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// Nazm's own code generator (R9, D15), built from `toolchain/codegen/` by this `nazm` once per
/// test process, into a directory of its own: what `NAZM_NZCG` names for `--backend nazm`.
pub(crate) fn nzcg() -> &'static Path {
    static NZCG: OnceLock<PathBuf> = OnceLock::new();
    NZCG.get_or_init(|| {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("nzcg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("nzcg");
        let main = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../toolchain/codegen/main.nz");
        let r = nazm(
            &dir,
            &[
                "build",
                "--no-cache",
                main.to_str().unwrap(),
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(r.code, Some(0), "nzcg does not build: {}", r.stderr);
        exe
    })
}

pub(crate) fn memory_line(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {}", r.stderr))
        .to_owned()
}

/// The program `files[0]` under the interpreter and built and run by each backend at each
/// optimisation level, every one with the memory report on.
pub(crate) fn everywhere(files: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let main = files[0].0;
    let mut interpreted = Command::new(NAZM);
    interpreted
        .current_dir(d.path())
        .args(["run", "--interpret", main])
        .env(nazm_lir::MEMORY_VARIABLE, "1");
    let mut out = vec![(
        "interpreter".to_owned(),
        run_bounded(&mut interpreted, "nazm run"),
    )];
    let mut backends = vec![
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ];
    // Nazm's own code generator, where it builds before 1.0 (its D13: macOS).
    if cfg!(target_os = "macos") {
        backends.push(("nazm", "nazm", "0"));
    }
    for (label, backend, opt) in backends {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let mut build = Command::new(NAZM);
        build.current_dir(d.path()).args([
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
            main,
            "-o",
            exe.to_str().unwrap(),
        ]);
        if backend == "nazm" {
            build.env(nazm_codegen_nazm::VARIABLE, nzcg());
        }
        let b = run_bounded(&mut build, "nazm build");
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        // In the program's directory, as the interpreter ran: a program that touches files
        // must find the same tree in every implementation (Gate 2).
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path()).env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected`, exits with `code`, writes the same memory report
/// — closure objects counted with everything else — and reclaims all it allocated.
pub(crate) fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) -> String {
    let runs = everywhere(files);
    let first = memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        let line = memory_line(r);
        assert_eq!(
            line, first,
            "{label} and the interpreter disagree about storage"
        );
        for kind in ["sequences", "strings", "channels", "closures"] {
            let part = line.split(kind).nth(1).unwrap_or("");
            let live = part.split_whitespace().nth(2).unwrap_or("");
            assert_eq!(live, "live=0", "{label}: {kind} not all reclaimed: {line}");
        }
    }
    first
}

pub(crate) fn agrees(src: &str, expected: &str) -> String {
    agrees_on(&[("prog.nz", src)], expected, 0)
}

pub(crate) fn check_codes(files: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let r = nazm(d.path(), &["check", "--json", files[0].0]);
    let mut codes: Vec<String> = Vec::new();
    for line in r.stdout.lines().chain(r.stderr.lines()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(c) = v["code"].as_str() {
            codes.push(c.to_owned());
        }
        if let Some(ds) = v["diagnostics"].as_array() {
            for d in ds {
                if let Some(c) = d["code"].as_str() {
                    codes.push(c.to_owned());
                }
            }
        }
    }
    codes
}

pub(crate) fn codes(src: &str) -> Vec<String> {
    check_codes(&[("prog.nz", src)])
}
