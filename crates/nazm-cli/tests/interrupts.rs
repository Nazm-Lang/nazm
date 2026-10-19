//! Gate 3: interrupts (`docs/user/spec.md`, *Boards*; `docs/internal/systems-domains.md` Part B).
//!
//! What a build shows on any host: a handler's shape, its uniqueness and the interrupt it names
//! are held to the board's manifest before anything is linked (`N0627`), a manifest's
//! `[interrupts]` to what this runtime can bind (`N0626`), and nothing but a board's build accepts
//! a handler at all (`N0392`). That interrupts are taken — the timer's at its period, a device
//! line's when it is pending, a handler's failure reported and the machine stopped — is the
//! ignored test at the end, built here and booted under QEMU in the `nazm-qemu:n86` image.

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

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// QEMU `virt`'s AArch64 board with its GICv2, a 1 ms timer, and line 40 named `poke`.
const BOARD: &str = "name = \"virt, with its GIC\"
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

[interrupts.lines]
poke = 40
";

/// The timer counts; `main` waits for five ticks.
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

/// Line 40 made pending through the distributor's set-pending register: its handler, given the
/// device's authority, writes `!` to the UART and records that it ran.
const POKE: &str = "static SEEN: Atomic = atomic_new(0);

@interrupt(\"poke\")
fn poked(m: MmioCap) -> Int ! {} {
    mmio_write8(150994944, 33);
    atomic_store(SEEN, 1)
}

fn main(m: MmioCap) -> Int ! {} {
    mmio_write32(134218244, 256);
    while atomic_load(SEEN) == 0 {
        0;
    }
    40
}
";

/// A handler whose addition overflows: reported as `main`'s failure would be, status 2.
const FAILS: &str = "static BIG: Atomic = atomic_new(9223372036854775807);

@interrupt(\"timer\")
fn tick() -> Int ! {} { atomic_add(BIG, 1) }

fn main(m: MmioCap) -> Int ! {} {
    while true {
        0;
    }
    0
}
";

/// `src` built for the AArch64 board with `manifest` as `board.toml`, into `o`.
fn build(src: &str, manifest: Option<&str>, opt: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut args = vec![
        "build",
        "--opt-level",
        opt,
        "--target",
        "aarch64-unknown-none",
        "--objects",
        "o",
    ];
    if let Some(m) = manifest {
        std::fs::write(d.path().join("board.toml"), m).unwrap();
        args.extend(["--board", "board.toml"]);
    }
    args.push("prog.nz");
    let r = nazm(d.path(), &args);
    (d, r)
}

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
fn a_handler_is_a_function_the_board_can_call_and_one_for_each_interrupt() {
    const MAIN: &str = "fn main(m: MmioCap) -> Int ! {} { 0 }\n";
    for handler in [
        // A parameter that is not the device's authority, a result that is not an `Int`, an
        // effect, no declared effect set, a type parameter.
        "@interrupt(\"timer\")\nfn h(n: Int) -> Int ! {} { n }\n",
        "@interrupt(\"timer\")\nfn h() -> Bool ! {} { true }\n",
        "@interrupt(\"timer\")\nfn h(io: IoCap) -> Int ! {io} { 0 }\n",
        "@interrupt(\"timer\")\nfn h() -> Int { 0 }\n",
        "@interrupt(\"timer\")\nfn h[T]() -> Int ! {} { 0 }\n",
        // Two for one interrupt.
        "@interrupt(\"timer\")\nfn h() -> Int ! {} { 0 }\n@interrupt(\"timer\")\nfn g() -> Int ! {} { 0 }\n",
    ] {
        assert_eq!(
            refused(&format!("{handler}{MAIN}")),
            ["N0627"],
            "{handler}"
        );
    }
    for misuse in [
        "@interrupt(\"timer\")\nstruct P { x: Int, }\n",
        "@interrupt\nfn h() -> Int ! {} { 0 }\n",
        "@interrupt(\"two words\")\nfn h() -> Int ! {} { 0 }\n",
    ] {
        assert_eq!(refused(&format!("{misuse}{MAIN}")), ["N0619"], "{misuse}");
    }
}

#[test]
fn a_handler_names_an_interrupt_the_boards_manifest_has() {
    // A board with no interrupts at all.
    let (_d, r) = build(TICKS, None, "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0627"), "{}", r.stderr);
    assert!(r.stderr.contains("has no interrupts"), "{}", r.stderr);
    // A name the manifest does not give.
    let (_d, r) = build(&TICKS.replace("\"timer\"", "\"uart\""), Some(BOARD), "0");
    assert!(r.stderr.contains("N0627"), "{}", r.stderr);
    assert!(r.stderr.contains("`uart`"), "{}", r.stderr);
    // The timer, on a board whose manifest gives it no period.
    let (_d, r) = build(
        TICKS,
        Some(&BOARD.replace("timer_period_us = 1000\n", "")),
        "0",
    );
    assert!(r.stderr.contains("timer_period_us"), "{}", r.stderr);
    // And every handler that does name one builds.
    for src in [TICKS, POKE, FAILS] {
        let (_d, r) = build(src, Some(BOARD), "0");
        assert_eq!(r.code, Some(0), "{}", r.stderr);
    }
}

#[test]
fn a_manifests_interrupts_are_a_gicv2_on_an_aarch64_board() {
    for (from, to, key) in [
        (
            "controller = \"gicv2\"",
            "controller = \"gicv3\"",
            "`interrupts.controller`",
        ),
        (
            "timer_period_us = 1000",
            "timer_period_us = 0",
            "`interrupts.timer_period_us`",
        ),
        ("poke = 40", "poke = 27", "`interrupts.lines.poke`"),
        ("poke = 40", "poke = 3", "`interrupts.lines.poke`"),
        ("poke = 40", "timer = 41", "`interrupts.lines.timer`"),
        (
            "cpu = 0x8010000",
            "cpu = 0x8010000\nvector = 1",
            "`interrupts.vector`",
        ),
    ] {
        let (_d, r) = build(TICKS, Some(&BOARD.replace(from, to)), "0");
        assert_eq!(r.code, Some(1), "{to}: {r:?}");
        assert!(r.stderr.contains("N0626"), "{to}: {}", r.stderr);
        assert!(r.stderr.contains(key), "{to}: {}", r.stderr);
    }
    // A RISC-V board's machine-mode interrupts are not this runtime's.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), "fn main() -> Int { 0 }\n").unwrap();
    std::fs::write(
        d.path().join("board.toml"),
        "name = \"rv\"\nisa = \"riscv64\"\nload_address = 0x80000000\n\n[uart]\nkind = \
         \"ns16550a\"\naddress = 0x10000000\n\n[stop]\nkind = \"sifive-test\"\naddress = \
         0x100000\n\n[interrupts]\ncontroller = \"gicv2\"\ndistributor = 0\ncpu = 0\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "riscv64gc-unknown-none-elf",
            "--board",
            "board.toml",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    assert!(r.stderr.contains("`interrupts`"), "{}", r.stderr);
}

#[test]
fn a_handler_is_a_boards_alone() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), TICKS).unwrap();
    let built = nazm(d.path(), &["build", "prog.nz", "-o", "x"]);
    assert_eq!(built.code, Some(1), "{built:?}");
    assert!(built.stderr.contains("N0392"), "{}", built.stderr);
    assert!(
        built.stderr.contains("`@interrupt` on `tick`"),
        "{}",
        built.stderr
    );
    let ran = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert!(ran.stderr.contains("N0392"), "{}", ran.stderr);
}

/// The three programs built here for `BOARD`, at `-O0` and `-O2`, then linked and booted under
/// QEMU in the `nazm-qemu:n86` image. The timer's five ticks: `5`, status 0. Line 40, made pending
/// by `main`: its handler's `!` on the UART, then `40`, status 0. A handler's overflow: its
/// diagnostic and status 2, as `main`'s would be.
#[test]
#[ignore = "starts a container"]
fn interrupts_are_taken_under_qemu() {
    for opt in ["0", "2"] {
        for (src, code, stdout, stderr) in [
            (TICKS, 0, "5\n", None),
            (POKE, 0, "!40\n", None),
            (FAILS, 2, "", Some("N0400")),
        ] {
            let (d, r) = build(src, Some(BOARD), opt);
            assert_eq!(r.code, Some(0), "{}", r.stderr);
            let out = Command::new("docker")
                .args(["run", "--rm", "--network", "none", "-v"])
                .arg(format!("{}:/w", d.path().display()))
                .args([
                    "nazm-qemu:n86",
                    "sh",
                    "-c",
                    "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf \
                     && timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic \
                     none -semihosting -kernel x.elf",
                ])
                .output()
                .unwrap();
            let got = String::from_utf8_lossy(&out.stdout);
            assert_eq!(out.status.code(), Some(code), "-O{opt}: {out:?}");
            match stderr {
                None => assert_eq!(got, stdout, "-O{opt}"),
                Some(c) => assert!(got.contains(c), "-O{opt}: {got}"),
            }
        }
    }
}
