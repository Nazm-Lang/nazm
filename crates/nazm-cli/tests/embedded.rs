//! Gate 3: the embedded domain's reference workload and `@std/hal` (`docs/internal/systems-domains.md` Part
//! C; `library/std/hal.nz`).
//!
//! A traffic light — red, green, yellow, each held for two of the board timer's ticks, announced on
//! the UART and read back from its pins — written once in `embedded/light.nz`, generic over the
//! HAL's `Uart`, `Pin` and `Timer`. Two boards supply the impls in their own `main`: QEMU's AArch64
//! `virt` (a PL011, a PL061's lines, the GIC's timer) and the MPS2 Cortex-M3 (the CMSDK UART, the
//! FPGA LED register's bits, SysTick). Any host builds both; the ignored test boots both under QEMU
//! in the `nazm-qemu:n86` image. Emulated boards only: nothing here has run on hardware.

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/embedded")
}

/// Each board: its `main`, its manifest, its target, and how QEMU boots it.
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

/// The workload for `board`, built at `opt` into `o` in a fresh directory.
fn build(board: &str, target: &str, opt: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    for f in ["light.nz", &format!("{board}.nz"), &format!("{board}.toml")] {
        std::fs::copy(fixtures().join(f), d.path().join(f)).unwrap();
    }
    let r = nazm(
        d.path(),
        &[
            "build",
            "--opt-level",
            opt,
            "--target",
            target,
            "--board",
            &format!("{board}.toml"),
            "--objects",
            "o",
            &format!("{board}.nz"),
        ],
    );
    (d, r)
}

#[test]
fn one_traffic_light_builds_for_both_boards_through_the_hal() {
    for (board, target, _) in BOARDS {
        let (_d, r) = build(board, target, "0");
        assert_eq!(r.code, Some(0), "{board}: {}", r.stderr);
    }
    // The device code is generic and names no board: the impls are in each `main`.
    let light = std::fs::read_to_string(fixtures().join("light.nz")).unwrap();
    for device in ["Pl011", "Pl061", "CmsdkUart", "FpgaLed", "mmio_"] {
        assert!(!light.contains(device), "light.nz names {device}");
    }
}

#[test]
fn a_cortex_m_board_has_no_heap() {
    let manifest = std::fs::read_to_string(fixtures().join("mps2.toml")).unwrap();
    let (d, _) = build("mps2", "thumbv7m-none-eabi", "0");
    std::fs::write(
        d.path().join("mps2.toml"),
        format!("{manifest}\n[heap]\nsize = 4096\n"),
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "thumbv7m-none-eabi",
            "--board",
            "mps2.toml",
            "--objects",
            "h",
            "mps2.nz",
        ],
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0626"), "{}", r.stderr);
    assert!(r.stderr.contains("`heap`"), "{}", r.stderr);
}

/// Booted under QEMU in the `nazm-qemu:n86` image at `-O0` and `-O2`: on both boards the light goes
/// red, green, yellow twice — every pin reading back what was written, or a `!` would be printed —
/// and `main` answers its six steps.
#[test]
#[ignore = "starts a container"]
fn the_traffic_light_runs_on_both_boards_under_qemu() {
    for opt in ["0", "2"] {
        for (board, target, qemu) in BOARDS {
            let (d, r) = build(board, target, opt);
            assert_eq!(r.code, Some(0), "{}", r.stderr);
            let out = Command::new("docker")
                .args(["run", "--rm", "--network", "none", "-v"])
                .arg(format!("{}:/w", d.path().display()))
                .args([
                    "nazm-qemu:n86",
                    "sh",
                    "-c",
                    &format!(
                        "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf \
                         && timeout 20 {qemu} x.elf"
                    ),
                ])
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(0), "{board} -O{opt}: {out:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "RGYRGY\n6\n",
                "{board} -O{opt}"
            );
        }
    }
}
