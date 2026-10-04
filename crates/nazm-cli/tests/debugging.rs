//! N48: debugging, inspection and profiling surfaces.
//!
//! `docs/architecture.md` §7.50. What a build with `--debug` carries and a debugger reads —
//! checked with `lldb` where it is installed, statically (breakpoints resolved by function and by
//! file and line) so no process has to be launched under a debugger; that `--debug` changes
//! nothing a program does and nothing an ordinary build writes; that `nazm inspect` is one
//! deterministic document of the program's facts; and that `--timings` accounts for every phase.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const PROG: &str = "fn compute(n: Int) -> Int {\n    let a = n * 2;\n    let b = a + 1;\n    b\n}\nfn main() -> Int {\n    compute(20)\n}\n";

fn nazm(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROG).unwrap();
    d
}

#[test]
fn a_debug_build_runs_as_an_ordinary_one_and_an_ordinary_one_carries_no_debug_information() {
    let d = setup();
    let plain = nazm(
        d.path(),
        &["build", "--no-cache", "--emit-ir", "prog.nz", "-o", "plain"],
    );
    assert!(plain.status.success(), "{}", err(&plain));
    let debug = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--debug",
            "--emit-ir",
            "prog.nz",
            "-o",
            "debug",
        ],
    );
    assert!(debug.status.success(), "{}", err(&debug));
    let a = common::bounded_exe(&d.path().join("plain"))
        .output()
        .unwrap();
    let b = common::bounded_exe(&d.path().join("debug"))
        .output()
        .unwrap();
    assert_eq!((a.status.code(), out(&a)), (b.status.code(), out(&b)));
    assert_eq!(out(&a), "41\n");
    let plain_ir = std::fs::read_to_string(d.path().join("plain.m0.ll")).unwrap();
    let debug_ir = std::fs::read_to_string(d.path().join("debug.m0.ll")).unwrap();
    assert!(
        !plain_ir.contains("!dbg") && !plain_ir.contains("nz.loc"),
        "{plain_ir}"
    );
    for want in [
        "DISubprogram(name: \"compute\"",
        "DISubprogram(name: \"main\"",
        "DILocation(line: 3, column:",
        "emissionKind: FullDebug",
    ] {
        assert!(debug_ir.contains(want), "{want}: {debug_ir}");
    }
    // Every instruction of a function with a subprogram has a location — except its prologue
    // (N58): the slots and the stores into them carry none, and the entry block's branch takes
    // the first statement's, so a breakpoint on the function stops after its slots are written.
    let mut inside = false;
    let mut prologue = false;
    for line in debug_ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains("!dbg");
            prologue = inside;
            continue;
        }
        if line == "}" {
            inside = false;
            continue;
        }
        let is_instruction = line.starts_with("  ")
            && !line.starts_with("    ")
            && !line.trim_start().starts_with(';')
            && !line.trim_end().ends_with('[');
        if inside && is_instruction {
            let call = line.contains(" call ") || line.trim_start().starts_with("call ");
            if prologue && line.trim_start().starts_with("br ") {
                assert!(
                    line.contains("!dbg"),
                    "the entry branch has no location: {line}"
                );
                prologue = false;
            } else if prologue && !call {
                assert!(
                    !line.contains("!dbg"),
                    "a prologue instruction with a line: {line}"
                );
            } else {
                assert!(
                    line.contains("!dbg"),
                    "an instruction without a location: {line}"
                );
            }
        }
    }
    if cfg!(target_os = "macos") {
        assert!(
            d.path().join("debug.dSYM").is_dir(),
            "no .dSYM beside the executable"
        );
    }
    // Cranelift writes debug information too since N91, and its debug build behaves as an
    // ordinary one.
    let c = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--debug",
            "--backend",
            "cranelift",
            "prog.nz",
            "-o",
            "c",
        ],
    );
    assert!(c.status.success(), "{}", err(&c));
    let r = common::bounded_exe(&d.path().join("c")).output().unwrap();
    assert_eq!((r.status.code(), out(&r)), (a.status.code(), out(&a)));
    if cfg!(target_os = "macos") {
        assert!(
            d.path().join("c.dSYM").is_dir(),
            "no .dSYM beside the executable"
        );
    }
}

#[test]
fn a_debugger_finds_nazm_functions_and_lines() {
    let Ok(probe) = Command::new("lldb").arg("--version").output() else {
        eprintln!("SKIP: no lldb on this machine");
        return;
    };
    if !probe.status.success() {
        eprintln!("SKIP: lldb does not run here");
        return;
    }
    let d = setup();
    let b = nazm(d.path(), &["build", "--debug", "prog.nz", "-o", "prog"]);
    assert!(b.status.success(), "{}", err(&b));
    // Static queries only: resolving a breakpoint reads the debug information and launches
    // nothing, so it cannot wait on a debugger's authorisation.
    let o = Command::new("lldb")
        .current_dir(d.path())
        .args([
            "--batch",
            "-o",
            "breakpoint set -n compute",
            "-o",
            "breakpoint set -f prog.nz -l 3",
            "-o",
            "image lookup -n compute -v",
            "./prog",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(
        text.contains("compute") && text.contains("at prog.nz:1"),
        "{text}"
    );
    assert!(
        text.contains("at prog.nz:3:"),
        "a breakpoint by line resolves to that line: {text}"
    );
    assert!(
        text.contains("CompileUnit") && text.contains("prog.nz"),
        "{text}"
    );
}

#[test]
fn inspect_is_one_deterministic_document_of_the_program_s_facts() {
    let d = setup();
    let a = nazm(d.path(), &["inspect", "prog.nz"]);
    assert!(a.status.success(), "{}", err(&a));
    let b = nazm(d.path(), &["inspect", "prog.nz"]);
    assert_eq!(out(&a), out(&b), "the same bytes every time");
    let v: serde_json::Value = serde_json::from_str(out(&a).trim()).unwrap();
    assert_eq!(v["schema"], "nazm.inspect/2");
    assert_eq!(v["mir"]["validated"], true);
    for key in [
        "compiler",
        "semantic_epoch",
        "interface",
        "runtime_abi",
        "cranelift",
        "target",
    ] {
        assert!(!v["toolchain"][key].is_null(), "{key}: {v}");
    }
    let defs = v["definitions"].as_array().unwrap();
    let compute = defs
        .iter()
        .find(|d| d["identity"] == "prog.nz::fn compute")
        .unwrap();
    assert_eq!(compute["params"], serde_json::json!(["Int"]));
    assert_eq!(compute["effects"]["required"], serde_json::json!([]));
    assert_eq!(compute["native"][0]["digest"].as_str().unwrap().len(), 64);
    assert!(
        !out(&a).contains(&d.path().display().to_string()),
        "no absolute path"
    );
    // One definition, by name.
    let one = nazm(d.path(), &["inspect", "--def", "compute", "prog.nz"]);
    let v: serde_json::Value = serde_json::from_str(out(&one).trim()).unwrap();
    assert_eq!(v["definitions"].as_array().unwrap().len(), 1);
}

#[test]
fn timings_account_for_every_phase() {
    let d = setup();
    for backend in ["llvm", "cranelift"] {
        let o = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--timings",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                "t",
            ],
        );
        assert!(o.status.success(), "{}", err(&o));
        let line = err(&o)
            .lines()
            .find(|l| l.contains("nazm.timings/1"))
            .unwrap()
            .to_owned();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let phases: Vec<&str> = v["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["phase"].as_str().unwrap())
            .collect();
        assert_eq!(
            phases,
            [
                "load",
                "front_end",
                "mir",
                "lir",
                "codegen",
                "native_compile",
                "link"
            ],
            "{backend}"
        );
        let sum: f64 = v["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["ms"].as_f64().unwrap())
            .sum();
        assert!(sum <= v["total_ms"].as_f64().unwrap() + 0.01, "{line}");
    }
}
