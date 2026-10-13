//! Gate 3: realtime — the board's fixed-priority scheduler and the strengthened `realtime` profile
//! (`docs/spec.md`, *Boards*, *Restriction profiles*; `docs/systems-domains.md` Part D).
//!
//! What a build shows on any host: a task's shape, its arguments and the timer it needs are held
//! before anything is linked, nothing but a board accepts one, and `realtime` refuses what has no
//! static stack bound or allocates. That the scheduler runs the higher priority first, releases each
//! task once a period and reports a missed deadline is the ignored test at the end, booted under
//! QEMU on both boards in the `nazm-qemu:n86` image. No execution time is bounded, and nothing here
//! is called hard real-time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/realtime_tasks")
}

const BOARDS: [(&str, &str, &str); 2] = [
    (
        "virt",
        "aarch64-unknown-none",
        "qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel",
    ),
    (
        "mps2",
        "thumbv7m-none-eabi",
        "qemu-system-arm -M mps2-an385 -cpu cortex-m3 -nographic -net none -semihosting -kernel",
    ),
];

/// `src` built for `board` (its manifest, or none) at `opt`, into `o`.
fn build(
    src: &str,
    board: &str,
    target: &str,
    manifest: bool,
    opt: &str,
) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut args = vec![
        "build",
        "--opt-level",
        opt,
        "--target",
        target,
        "--objects",
        "o",
    ];
    let toml = format!("{board}.toml");
    if manifest {
        std::fs::copy(fixtures().join(&toml), d.path().join(&toml)).unwrap();
        args.extend(["--board", &toml]);
    }
    args.push("prog.nz");
    let r = nazm(d.path(), &args);
    (d, r)
}

/// The codes `nazm check` refuses a program with, under `profile` where one is given.
fn refused(src: &str, profile: Option<&str>) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut args = vec!["check", "--json"];
    if let Some(p) = profile {
        args.extend(["--profile", p]);
    }
    args.push("prog.nz");
    let r = nazm(d.path(), &args);
    assert_eq!(r.code, Some(1), "expected a refusal: {r:?}\n{src}");
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|d| d["severity"] == "error")
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

const MAIN: &str = "fn main(m: MmioCap) -> Int ! {} { 0 }\n";

#[test]
fn a_task_is_a_function_the_scheduler_can_call_with_a_priority_and_a_period() {
    for task in [
        "@task(priority = \"1\", period = \"2\")\nfn t(n: Int) -> Int ! {} { n }\n",
        "@task(priority = \"1\", period = \"2\")\nfn t() -> Int { 0 }\n",
        "@task(priority = \"1\", period = \"2\")\n@on_failure\nfn t(code: Int) -> Int ! {} { 0 }\n",
    ] {
        assert!(
            refused(&format!("{task}{MAIN}"), None).contains(&"N0627".to_owned()),
            "{task}"
        );
    }
    for misuse in [
        "@task\nfn t() -> Int ! {} { 0 }\n",
        "@task(priority = \"256\", period = \"2\")\nfn t() -> Int ! {} { 0 }\n",
        "@task(priority = \"1\", period = \"0\")\nfn t() -> Int ! {} { 0 }\n",
        "@task(priority = \"1\")\nfn t() -> Int ! {} { 0 }\n",
        "@task(priority = \"1\", period = \"2\", deadline = \"1\")\nfn t() -> Int ! {} { 0 }\n",
        "@task(priority = \"1\", period = \"2\")\nstruct P { x: Int, }\n",
    ] {
        assert_eq!(
            refused(&format!("{misuse}{MAIN}"), None),
            ["N0619"],
            "{misuse}"
        );
    }
}

#[test]
fn a_task_needs_a_boards_timer_to_itself() {
    let periodic = std::fs::read_to_string(fixtures().join("periodic.nz")).unwrap();
    // No manifest: the built-in board has no interrupts.
    let (_d, r) = build(&periodic, "virt", "aarch64-unknown-none", false, "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0627"), "{}", r.stderr);
    assert!(r.stderr.contains("no timer"), "{}", r.stderr);
    // A program that handles the timer itself.
    let both = format!("{periodic}\n@interrupt(\"timer\")\nfn tick() -> Int ! {{}} {{ 0 }}\n");
    let (_d, r) = build(&both, "virt", "aarch64-unknown-none", true, "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0627"), "{}", r.stderr);
    // And each board builds the periodic tasks with its manifest.
    for (board, target, _) in BOARDS {
        let (_d, r) = build(&periodic, board, target, true, "0");
        assert_eq!(r.code, Some(0), "{board}: {}", r.stderr);
    }
    // Nothing but a board has a scheduler.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), &periodic).unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert!(r.stderr.contains("N0392"), "{}", r.stderr);
}

#[test]
fn realtime_refuses_what_has_no_stack_bound_or_allocates() {
    let cycle = "fn down(n: Int) -> Int ! {} { if n == 0 { 0 } else { down(n - 1) } }\n\
                 fn main() -> Int ! {} { down(3) }\n";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.nz"), cycle).unwrap();
    let r = nazm(d.path(), &["check", "--profile", "realtime", "a.nz"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0510"), "{}", r.stderr);
    assert!(r.stderr.contains("`bounded-stack`"), "{}", r.stderr);
    std::fs::write(
        d.path().join("b.nz"),
        "fn main() -> Int ! {} { let v = vec_new[Int](); vec_push(v, 1); vec_len(v) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "realtime", "b.nz"]);
    assert!(r.stderr.contains("`no-heap`"), "{}", r.stderr);
    // The periodic tasks are realtime's.
    std::fs::copy(fixtures().join("periodic.nz"), d.path().join("p.nz")).unwrap();
    let r = nazm(d.path(), &["check", "--profile", "realtime", "p.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// Booted under QEMU on both boards at `-O0` and `-O2`: the higher priority's task has run ten
/// times when the lower's fourth run ends the loop at tick 20 (`1004`, status 0), and a task whose
/// run outlasts its period is reported (`N0414`, status 2).
#[test]
#[ignore = "starts a container"]
fn the_scheduler_runs_by_priority_and_reports_a_missed_deadline_under_qemu() {
    let periodic = std::fs::read_to_string(fixtures().join("periodic.nz")).unwrap();
    let overrun = std::fs::read_to_string(fixtures().join("overrun.nz")).unwrap();
    for opt in ["0", "2"] {
        for (board, target, qemu) in BOARDS {
            for (src, code, want) in [(&periodic, 0, "1004\n"), (&overrun, 2, "N0414")] {
                let (d, r) = build(src, board, target, true, opt);
                assert_eq!(r.code, Some(0), "{}", r.stderr);
                let out = Command::new("docker")
                    .args(["run", "--rm", "--network", "none", "-v"])
                    .arg(format!("{}:/w", d.path().display()))
                    .args([
                        "nazm-qemu:n86",
                        "sh",
                        "-c",
                        &format!(
                            "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) \
                             -o x.elf && timeout 30 {qemu} x.elf"
                        ),
                    ])
                    .output()
                    .unwrap();
                let got = String::from_utf8_lossy(&out.stdout);
                assert_eq!(out.status.code(), Some(code), "{board} -O{opt}: {out:?}");
                assert!(got.contains(want), "{board} -O{opt}: {got}");
            }
        }
    }
}
