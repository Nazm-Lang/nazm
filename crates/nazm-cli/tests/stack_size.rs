//! R1b, L14: `nazm build --stack-size` (`docs/user/spec.md`, *Native recursion, and the stack it runs
//! on*).
//!
//! A program that recurses 250,000 calls deep, in `main` and in a task, meets `N0408` on the
//! default stacks and finishes on the one it asks for — under LLVM and Cranelift, on the pool and
//! on a thread per task. Asking for too little is still `N0408`, reported as a failure: `main`
//! runs on a thread of its own, and how it failed has to reach the report. A board or WebAssembly
//! target, and a size that is not one, are refused.
//!
//! A sized program runs under the many-task address-space ceiling: `main`, the task and every pool
//! worker each reserve the stack asked for, which a 6-CPU container's pool alone would take past
//! the default ceiling.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::run_bounded;

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const DEEP: &str = "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }
fn task(c: Chan) -> Int { chan_send(c, down(250000)); 0 }
fn main(t: SpawnCap) -> Int ! {spawn} {
    let c = chan_new(1);
    scope { spawn task(c); }
    let got = ints_new();
    chan_recv(c, got);
    down(250000) + ints_get(got, 0)
}
";

fn build(dir: &std::path::Path, backend: &str, size: Option<&str>) -> std::path::PathBuf {
    let exe = dir.join(crate::common::exe_name(format!(
        "{backend}-{}",
        size.unwrap_or("default")
    )));
    let mut args = vec![
        "build",
        "--backend",
        backend,
        "deep.nz",
        "-o",
        exe.to_str().unwrap(),
    ];
    if let Some(s) = size {
        args.extend(["--stack-size", s]);
    }
    let b = run_bounded(
        Command::new(NAZM).current_dir(dir).args(&args),
        "nazm build",
    );
    assert_eq!(b.code, Some(0), "{backend} {size:?}: {}", b.stderr);
    exe
}

#[test]
fn the_stack_asked_for_carries_a_recursion_the_default_does_not() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("deep.nz"), DEEP).unwrap();
    for backend in ["llvm", "cranelift"] {
        let plain = build(d.path(), backend, None);
        let r = run_bounded(&mut common::bounded_exe(&plain), backend);
        assert!(r.stderr.contains("error[N0408]"), "{backend}: {r:?}");
        let sized = build(d.path(), backend, Some("64M"));
        let small = build(d.path(), backend, Some("4M"));
        for scheduler in ["pool", "threads"] {
            let r = run_bounded(
                common::bounded_exe_within(&sized, common::TASKS_ADDRESS_SPACE_KB)
                    .env("NAZM_SCHEDULER", scheduler),
                backend,
            );
            assert!(
                r.stdout.starts_with("500000"),
                "{backend} {scheduler}: {r:?}"
            );
            let r = run_bounded(
                common::bounded_exe_within(&small, common::TASKS_ADDRESS_SPACE_KB)
                    .env("NAZM_SCHEDULER", scheduler),
                backend,
            );
            assert_eq!(r.code, Some(2), "{backend} {scheduler}: {r:?}");
            assert!(r.stdout.is_empty(), "{backend} {scheduler}: {r:?}");
            assert!(
                r.stderr.contains("error[N0408]"),
                "{backend} {scheduler}: {r:?}"
            );
        }
    }
}

#[test]
fn a_stack_size_is_a_hosted_programs_and_a_size() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("deep.nz"), DEEP).unwrap();
    let r = run_bounded(
        Command::new(NAZM).current_dir(d.path()).args([
            "build",
            "--target",
            "wasm32-wasip1",
            "--stack-size",
            "1M",
            "deep.nz",
            "-o",
            "w.wasm",
        ]),
        "nazm build",
    );
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("`--stack-size`"), "{r:?}");
    for bad in ["12", "1x", "8G", "M"] {
        let r = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--stack-size",
                bad,
                "deep.nz",
                "-o",
                "x",
            ]),
            "nazm build",
        );
        assert_ne!(r.code, Some(0), "{bad}: {r:?}");
    }
}
