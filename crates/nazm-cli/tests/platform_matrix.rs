//! N95: the support matrix — every target, under each backend, run-verified, run-verified under
//! Wine (Gate 3, Windows only and never a Windows host's evidence), compile-only or unsupported,
//! and nothing in between.
//!
//! `docs/architecture.md` §7.96. The matrix is one table in `nazm-lir`, printed by `nazm inspect`;
//! this holds the capability matrix's table to it cell for cell, every run-verified or
//! compile-only cell to evidence the tree contains, every target the compiler accepts to a row,
//! and each recorded LLVM CPU to what the C compiler driver resolves for the triple.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn support() -> (Vec<Value>, Vec<Value>) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), "fn main() -> Int { 0 }\n").unwrap();
    let o = Command::new(NAZM)
        .current_dir(d.path())
        .args(["inspect", "p.nz"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    (
        v["toolchain"]["support"].as_array().unwrap().clone(),
        v["toolchain"]["targets"].as_array().unwrap().clone(),
    )
}

/// Whether the tree's Rust sources define a function of this name.
fn defined(name: &str) -> bool {
    let needle = format!("fn {name}(");
    let mut stack = vec![root().join("crates")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs")
                && std::fs::read_to_string(&p).unwrap().contains(&needle)
            {
                return true;
            }
        }
    }
    false
}

#[test]
fn every_accepted_target_has_a_row_and_every_cell_one_of_five_answers() {
    let (cells, targets) = support();
    let triples: Vec<&str> = cells
        .iter()
        .map(|c| c["triple"].as_str().unwrap())
        .collect();
    for t in &targets {
        assert!(
            triples.contains(&t["triple"].as_str().unwrap()),
            "{t} has no row"
        );
    }
    assert_eq!(triples.len(), targets.len());
    for c in &cells {
        for backend in ["llvm", "cranelift"] {
            let status = c[backend]["status"].as_str().unwrap();
            assert!(
                [
                    "run-verified",
                    "run-verified under Wine",
                    // Gate 3: static Bionic on an aarch64 Linux kernel, never an Android device.
                    "run-verified on a Linux kernel",
                    "compile-only",
                    "unsupported"
                ]
                .contains(&status),
                "{c}"
            );
            let evidence = c[backend]["evidence"].as_str().unwrap();
            assert!(!evidence.is_empty(), "{c}");
            // Evidence the tree holds: a test it defines, or a contained command.
            if status != "unsupported" && !evidence.starts_with("cargo xtask contained ") {
                assert!(defined(evidence), "{evidence}: no such test");
            }
        }
    }
}

#[test]
fn the_capability_matrix_states_exactly_what_the_compiler_does() {
    let (cells, _) = support();
    let text = std::fs::read_to_string(root().join("docs/capability-matrix.md")).unwrap();
    // The N95 table: `| triple | LLVM | Cranelift | CPU (LLVM / Cranelift) | debugger |`.
    for c in &cells {
        let triple = c["triple"].as_str().unwrap();
        let row = text
            .lines()
            .find(|l| l.starts_with(&format!("| `{triple}` |")))
            .unwrap_or_else(|| panic!("no row for {triple} in docs/capability-matrix.md"));
        let cols: Vec<&str> = row.split('|').map(str::trim).collect();
        assert_eq!(cols[2], c["llvm"]["status"].as_str().unwrap(), "{row}");
        assert_eq!(cols[3], c["cranelift"]["status"].as_str().unwrap(), "{row}");
        assert_eq!(
            cols[4],
            format!(
                "{} / {}",
                c["llvm"]["cpu"].as_str().unwrap(),
                c["cranelift"]["cpu"].as_str().unwrap()
            ),
            "{row}"
        );
    }
    // No row says anything vaguer.
    assert!(!text.contains("| supported |"));
}

// The table records Apple clang's choices: elsewhere ignored, not a pass (Q1-D-02).
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Apple clang only")]
fn each_llvm_cpu_is_the_one_the_driver_resolves() {
    let probe = Command::new("clang").arg("--version").output();
    if common::skip_without(probe.is_ok(), "no clang") {
        return;
    }
    let Ok(probe) = probe else { return };
    assert!(probe.status.success());
    // The CPU is the driver's choice, and the table records Apple clang's, the host's. Another
    // clang chooses otherwise — clang 19, in the Linux image, resolves `core2` for
    // `x86_64-apple-darwin` where Apple clang 21 resolves `penryn` (found by N100's gate) — so
    // the check is made against the driver the table describes, and nowhere else.
    if common::skip_without(
        String::from_utf8_lossy(&probe.stdout).contains("Apple clang"),
        "Apple clang only; this driver is another",
    ) {
        return;
    }
    let (cells, _) = support();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("e.c"), "").unwrap();
    for c in &cells {
        let triple = c["triple"].as_str().unwrap();
        let mut args: Vec<&str> = if triple == "riscv64gc-unknown-none-elf" {
            vec![
                "-target",
                "riscv64-unknown-elf",
                "-march=rv64gc",
                "-mabi=lp64d",
            ]
        } else {
            vec!["-target", triple]
        };
        args.extend(["-###", "-c", "e.c"]);
        let o = Command::new("clang")
            .current_dir(d.path())
            .args(&args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&o.stderr);
        let cpu = text
            .split("\"-target-cpu\" \"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap_or("");
        assert_eq!(cpu, c["llvm"]["cpu"].as_str().unwrap(), "{triple}");
    }
}
