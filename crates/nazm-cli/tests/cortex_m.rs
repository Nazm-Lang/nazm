//! Gate 3: the Cortex-M board (`docs/user/spec.md`, *A freestanding target*; `docs/internal/systems-domains.md`
//! Part C).
//!
//! What a build shows on any host: a program for `thumbv7m-none-eabi` is built to objects whose
//! linker script puts the M profile's vector table first, at address 0; and what the Cortex-M3 has
//! no instruction for — floating point, a 64-bit device access — is refused by name. That the image
//! boots, and that 64-bit division, overflow and atomics mean on a 32-bit core what they mean
//! everywhere, is the ignored test at the end, run by hand under QEMU's `mps2-an385` in the
//! `nazm-qemu:n86` image.

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
const TARGET: &str = "thumbv7m-none-eabi";

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// 64-bit arithmetic on a 32-bit core: signed division and remainder, the largest `Int` divided,
/// the largest square that fits, a static atomic, and a multiplication that overflows.
const ARITHMETIC: &str = "static HITS: Atomic = atomic_new(5);
static UART: Int = 1073758208;

fn show(m: MmioCap, n: Int) -> Int ! {} {
    let mut v = n;
    if v < 0 { mmio_write8(UART, 45); v = 0 - v; }
    let mut d = 1000000000000000000;
    let mut started = false;
    while d > 0 {
        let q = v / d;
        if q > 0 || started || d == 1 { mmio_write8(UART, 48 + q); started = true; }
        v = v % d;
        d = d / 10;
    }
    mmio_write8(UART, 10)
}

fn main(m: MmioCap) -> Int ! {} {
    show(m, -7 / 2);
    show(m, -7 % 2);
    show(m, 9223372036854775807 / 3);
    show(m, 3037000499 * 3037000499);
    atomic_add(HITS, 10);
    show(m, atomic_swap(HITS, 1));
    show(m, if atomic_compare_swap(HITS, 1, 2) { atomic_load(HITS) } else { 0 });
    3037000500 * 3037000500
}
";

fn build(src: &str, opt: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--opt-level",
            opt,
            "--target",
            TARGET,
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    (d, r)
}

#[test]
fn a_cortex_m_build_puts_its_vector_table_first() {
    let (d, r) = build(ARITHMETIC, "0");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    assert!(script.contains(". = 0x0;"), "{script}");
    assert!(
        script.contains(".text : { KEEP(*(.vectors)) *(.text.start)"),
        "{script}"
    );
    let link = std::fs::read_to_string(d.path().join("o/link.txt")).unwrap();
    assert!(link.contains("target thumbv7m-none-eabi"), "{link}");
}

#[test]
fn what_a_cortex_m3_has_no_instruction_for_is_refused_by_name() {
    for (src, says) in [
        (
            "fn main(m: MmioCap) -> Int ! {} { let x = 1.5; Int(x * 2.0) }\n",
            "no FPU",
        ),
        (
            "fn half(x: Float64) -> Float64 { x / 2.0 }\nfn main(m: MmioCap) -> Int ! {} { 0 }\n",
            "no FPU",
        ),
        (
            "fn main(m: MmioCap) -> Int ! {} { mmio_read64(1073758208) }\n",
            "`mmio_read64`",
        ),
        (
            "fn main(m: MmioCap) -> Int ! {} { mmio_write64(1073758208, 1) }\n",
            "`mmio_write64`",
        ),
    ] {
        let (_d, r) = build(src, "0");
        assert_eq!(r.code, Some(1), "{src}: {r:?}");
        assert!(r.stderr.contains("N0392"), "{src}: {}", r.stderr);
        assert!(r.stderr.contains(says), "{src}: {}", r.stderr);
    }
}

/// Built here at `-O0` and `-O2`, then linked and booted under QEMU's `mps2-an385` in the
/// `nazm-qemu:n86` image: every line the interpreter's arithmetic gives, the atomic's 15 and 2,
/// and the overflow's `N0400` with status 2.
#[test]
#[ignore = "starts a container"]
fn an_image_for_the_cortex_m_board_boots_under_qemu() {
    for opt in ["0", "2"] {
        let (d, r) = build(ARITHMETIC, opt);
        assert_eq!(r.code, Some(0), "{}", r.stderr);
        let out = Command::new("docker")
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{}:/w", d.path().display()))
            .args([
                "nazm-qemu:n86",
                "sh",
                "-c",
                "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf && \
                 timeout 20 qemu-system-arm -M mps2-an385 -cpu cortex-m3 -nographic -net none \
                 -semihosting -kernel x.elf",
            ])
            .output()
            .unwrap();
        let got = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(2), "-O{opt}: {out:?}");
        assert!(
            got.starts_with("-3\n-1\n3074457345618258602\n9223372030926249001\n15\n2\n"),
            "-O{opt}: {got}"
        );
        assert!(got.contains("N0400"), "-O{opt}: {got}");
    }
}

/// The Cortex-M board with its NVIC: SysTick every millisecond at the AN385's 25 MHz, and
/// external interrupt 3 named `poke`.
const NVIC_BOARD: &str = "name = \"MPS2 AN385, with its NVIC\"
isa = \"thumbv7m\"
load_address = 0x0

[uart]
kind = \"cmsdk\"
address = 0x40004000

[stop]
kind = \"semihosting\"

[interrupts]
controller = \"nvic\"
clock_hz = 25000000
timer_period_us = 1000

[interrupts.lines]
poke = 3
";

/// SysTick counts five ticks into a static atomic `main` waits on.
const TICKS: &str = "static TICKS: Atomic = atomic_new(0);

@interrupt(\"timer\")
fn tick() -> Int ! {} { atomic_add(TICKS, 1) }

fn main(m: MmioCap) -> Int ! {} {
    while atomic_load(TICKS) < 5 {
        0;
    }
    atomic_load(TICKS)
}
";

/// Line 3 made pending through the NVIC's set-pending register: its handler, given the device's
/// authority, writes `!`.
const POKE: &str = "static SEEN: Atomic = atomic_new(0);

@interrupt(\"poke\")
fn poked(m: MmioCap) -> Int ! {} {
    mmio_write8(1073758208, 33);
    atomic_store(SEEN, 1)
}

fn main(m: MmioCap) -> Int ! {} {
    mmio_write32(3758154240, 8);
    while atomic_load(SEEN) == 0 {
        0;
    }
    3
}
";

/// `src` for the Cortex-M board with `manifest` as `board.toml`.
fn build_on(src: &str, manifest: &str, opt: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    std::fs::write(d.path().join("board.toml"), manifest).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--opt-level",
            opt,
            "--target",
            TARGET,
            "--board",
            "board.toml",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    (d, r)
}

#[test]
fn a_cortex_m_boards_interrupts_are_an_nvic_and_systick() {
    for src in [TICKS, POKE] {
        let (_d, r) = build_on(src, NVIC_BOARD, "0");
        assert_eq!(r.code, Some(0), "{}", r.stderr);
    }
    for (from, to, key) in [
        (
            "controller = \"nvic\"",
            "controller = \"gicv2\"",
            "`interrupts.controller`",
        ),
        ("clock_hz = 25000000\n", "", "`interrupts.clock_hz`"),
        (
            "timer_period_us = 1000",
            "timer_period_us = 1000000",
            "`interrupts.timer_period_us`",
        ),
        ("poke = 3", "poke = 240", "`interrupts.lines.poke`"),
        (
            "clock_hz = 25000000",
            "clock_hz = 25000000\ndistributor = 1",
            "`interrupts.distributor`",
        ),
    ] {
        let (_d, r) = build_on(TICKS, &NVIC_BOARD.replace(from, to), "0");
        assert_eq!(r.code, Some(1), "{to}: {r:?}");
        assert!(r.stderr.contains("N0626"), "{to}: {}", r.stderr);
        assert!(r.stderr.contains(key), "{to}: {}", r.stderr);
    }
}

/// Booted under QEMU's `mps2-an385` in the `nazm-qemu:n86` image at `-O0` and `-O2`: SysTick's five
/// ticks (`5`), and line 3's handler (`!3`), each status 0.
#[test]
#[ignore = "starts a container"]
fn the_cortex_m_boards_interrupts_are_taken_under_qemu() {
    for opt in ["0", "2"] {
        for (src, want) in [(TICKS, "5\n"), (POKE, "!3\n")] {
            let (d, r) = build_on(src, NVIC_BOARD, opt);
            assert_eq!(r.code, Some(0), "{}", r.stderr);
            let out = Command::new("docker")
                .args(["run", "--rm", "--network", "none", "-v"])
                .arg(format!("{}:/w", d.path().display()))
                .args([
                    "nazm-qemu:n86",
                    "sh",
                    "-c",
                    "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf \
                     && timeout 20 qemu-system-arm -M mps2-an385 -cpu cortex-m3 -nographic -net \
                     none -semihosting -kernel x.elf",
                ])
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(0), "-O{opt}: {out:?}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), want, "-O{opt}");
        }
    }
}
