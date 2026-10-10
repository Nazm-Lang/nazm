//! Gate 3: the failure hook and the reference kernel-style workload (`docs/spec.md`, *Boards*;
//! `docs/systems-domains.md` Part B, *Reference workload*).
//!
//! The workload is one AArch64 image: a timer interrupt that counts ticks into a static atomic, a
//! UART driven by polling its flag register through MMIO, and a fault once three ticks have
//! passed, which the program's `@on_failure` hook sees by number before the machine stops. It is
//! held to the `kernel` profile — no tasks, no heap, no C, nothing that blocks — and built here;
//! the ignored test boots it under QEMU in the `nazm-qemu:n86` image. Nothing here claims an
//! operating system: it is a kernel-*style* image on an emulator.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// QEMU `virt`'s AArch64 board, its GICv2 and a 1 ms timer.
const BOARD: &str = "name = \"virt, the kernel workload's\"
isa = \"aarch64\"
load_address = 0x40080000

[uart]
kind = \"pl011\"
address = 0x9000000

[stop]
kind = \"semihosting\"

[interrupts]
controller = \"gicv2\"
distributor = 0x8000000
cpu = 0x8010000
timer_period_us = 1000
";

const KERNEL: &str = "static UART: Int = 150994944;
static TICKS: Atomic = atomic_new(0);

@interrupt(\"timer\")
fn tick() -> Int ! {} { atomic_add(TICKS, 1) }

// The PL011's flag register, 0x18 past its data register: bit 5 is \"transmit FIFO full\".
fn ready(m: MmioCap) -> Bool ! {} { (mmio_read32(UART + 24) & 32) == 0 }

fn put(m: MmioCap, c: Int) -> Int ! {} {
    while !ready(m) {
        0;
    }
    mmio_write8(UART, c)
}

@on_failure
fn halted(code: Int, m: MmioCap) -> Int ! {} {
    // `F` for the division the workload faults with, `?` for anything else.
    if code == 401 { put(m, 70) } else { put(m, 63) };
    put(m, 10)
}

fn main(m: MmioCap) -> Int ! {} {
    while atomic_load(TICKS) < 3 {
        0;
    }
    put(m, 75);
    put(m, 10);
    let t = atomic_load(TICKS);
    100 / (t - t)
}
";

/// The codes `nazm check` refuses a program with.
fn refused(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["check", "--json", "prog.nz"]);
    assert_eq!(r.code, Some(1), "expected a refusal: {r:?}\n{src}");
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|d| d["severity"] == "error")
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_failure_hook_takes_the_number_and_there_is_one() {
    const MAIN: &str = "fn main(m: MmioCap) -> Int ! {} { 0 }\n";
    for hook in [
        "@on_failure\nfn h() -> Int ! {} { 0 }\n",
        "@on_failure\nfn h(code: Bool) -> Int ! {} { 0 }\n",
        "@on_failure\nfn h(code: Int, n: Int) -> Int ! {} { 0 }\n",
        "@on_failure\nfn h(code: Int) -> Int { 0 }\n",
        "@on_failure\nfn h(code: Int) -> Int ! {} { 0 }\n@on_failure\nfn g(code: Int) -> Int ! {} { 0 }\n",
    ] {
        assert_eq!(refused(&format!("{hook}{MAIN}")), ["N0627"], "{hook}");
    }
    for misuse in [
        "@on_failure(\"x\")\nfn h(code: Int) -> Int ! {} { 0 }\n",
        "@on_failure\nstatic S: Int = 1;\n",
    ] {
        assert_eq!(refused(&format!("{misuse}{MAIN}")), ["N0619"], "{misuse}");
    }
    // A board's alone.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        format!("@on_failure\nfn h(code: Int) -> Int ! {{}} {{ code }}\n{MAIN}"),
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert!(r.stderr.contains("N0392"), "{}", r.stderr);
    assert!(r.stderr.contains("`@on_failure` on `h`"), "{}", r.stderr);
}

#[test]
fn the_workload_is_held_to_the_kernel_profile_and_builds_for_its_board() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("kernel.nz"), KERNEL).unwrap();
    std::fs::write(d.path().join("board.toml"), BOARD).unwrap();
    let r = nazm(d.path(), &["check", "--profile", "kernel", "kernel.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "board.toml",
            "--objects",
            "o",
            "kernel.nz",
        ],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// Built here at `-O0` and `-O2`, then linked and booted under QEMU in the `nazm-qemu:n86` image:
/// `K` once three ticks have passed, the division's diagnostic, the hook's `F` — it was handed
/// 401 — and the machine stopped with status 2.
#[test]
#[ignore = "starts a container"]
fn the_kernel_workload_boots_ticks_polls_and_fails_under_qemu() {
    for opt in ["0", "2"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("kernel.nz"), KERNEL).unwrap();
        std::fs::write(d.path().join("board.toml"), BOARD).unwrap();
        let r = nazm(
            d.path(),
            &[
                "build",
                "--opt-level",
                opt,
                "--target",
                "aarch64-unknown-none",
                "--board",
                "board.toml",
                "--objects",
                "o",
                "kernel.nz",
            ],
        );
        assert_eq!(r.code, Some(0), "{}", r.stderr);
        let out = Command::new("docker")
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{}:/w", d.path().display()))
            .args([
                "nazm-qemu:n86",
                "sh",
                "-c",
                "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf && \
                 timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none \
                 -semihosting -kernel x.elf",
            ])
            .output()
            .unwrap();
        let got = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(2), "-O{opt}: {out:?}");
        assert!(got.starts_with("K\nnazm: error[N0401]"), "-O{opt}: {got}");
        assert!(got.ends_with("F\n"), "-O{opt}: {got}");
    }
}
