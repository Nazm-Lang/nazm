//! Gate 3: board manifests (`docs/spec.md`, *Boards*; `docs/systems-domains.md` Part B).
//!
//! What a build shows on any host: a manifest's load address and stack reach the linker script and
//! the stack bound; every key it does not know, every value of the wrong kind or out of range and
//! an instruction set that is not the target's are refused by name (`N0626`), and `--board` for a
//! target with no board is the command line's mistake. That an image linked for a manifest's board
//! boots is the ignored test at the end, run by hand against the `nazm-qemu:n86` image.

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

const PROGRAM: &str = "fn main(m: MmioCap) -> Int ! {} { 7 }\n";

/// QEMU `virt`'s AArch64 board, loaded 1 MiB higher, with a 32 KiB stack.
const MOVED: &str = "name = \"virt, loaded higher\"
isa = \"aarch64\"
load_address = 0x40180000
stack = 0x8000

[uart]
kind = \"pl011\"
address = 0x9000000

[stop]
kind = \"semihosting\"
";

/// A build for the AArch64 board with `manifest` as `board.toml`.
fn build(manifest: &str, target: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    std::fs::write(d.path().join("board.toml"), manifest).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            target,
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
fn a_manifests_load_address_and_stack_reach_the_script_and_the_bound() {
    let (d, r) = build(MOVED, "aarch64-unknown-none");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    assert!(script.contains(". = 0x40180000;"), "{script}");
    assert!(script.contains(". = . + 0x8000;"), "{script}");
    let bounds = std::fs::read_to_string(d.path().join("o/bounds.json")).unwrap();
    assert!(bounds.contains("32768"), "{bounds}");
}

#[test]
fn every_built_in_board_is_a_manifest_that_builds_as_itself() {
    use nazm_lir::runtime::board::BOARDS;
    let b = &BOARDS[0];
    let (d, r) = build(&b.manifest(), &b.triple);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let with = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    let r = nazm(
        d.path(),
        &["build", "--target", &b.triple, "--objects", "p", "prog.nz"],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let without = std::fs::read_to_string(d.path().join("p/link.ld")).unwrap();
    assert_eq!(with, without);
}

#[test]
fn a_manifest_it_cannot_honour_is_refused_naming_the_key() {
    let cases: &[(&str, &str)] = &[
        ("color = \"red\"\n", "`color`"),
        ("isa = \"riscv64\"\n", "`isa`"),
        ("isa = \"mips\"\n", "`isa`"),
        ("stack = 0x10\n", "`stack`"),
        ("stack = 0x1001\n", "`stack`"),
        ("load_address = -1\n", "`load_address`"),
        ("load_address = \"high\"\n", "`load_address`"),
        (
            "[uart]\nkind = \"usb\"\naddress = 0x9000000\n",
            "`uart.kind`",
        ),
        (
            "[uart]\nkind = \"pl011\"\nbaud = 9600\naddress = 1\n",
            "`uart.baud`",
        ),
        (
            "[stop]\nkind = \"semihosting\"\naddress = 4\n",
            "`stop.address`",
        ),
        ("[stop]\nkind = \"sifive-test\"\n", "`stop.address`"),
        ("[heap]\nsize = 0\n", "`heap.size`"),
        ("[heap]\nsize = 4096\nalign = 8\n", "`heap.align`"),
        ("not toml at all [", "the file"),
    ];
    for (change, key) in cases {
        let manifest = edited(change);
        let (_d, r) = build(&manifest, "aarch64-unknown-none");
        assert_eq!(r.code, Some(1), "{change}: {r:?}");
        assert!(r.stderr.contains("N0626"), "{change}: {}", r.stderr);
        assert!(r.stderr.contains(key), "{change}: {}", r.stderr);
    }
    // Missing a table it needs.
    let (_d, r) = build(
        "name = \"x\"\nisa = \"aarch64\"\nload_address = 0x40080000\n[stop]\nkind = \"semihosting\"\n",
        "aarch64-unknown-none",
    );
    assert!(r.stderr.contains("`uart`"), "{}", r.stderr);
    // A RISC-V board stops through the SiFive test device, not semihosting.
    let rv = "name = \"rv\"\nisa = \"riscv64\"\nload_address = 0x80000000\n\n[uart]\nkind = \
              \"ns16550a\"\naddress = 0x10000000\n\n[stop]\nkind = \"semihosting\"\n";
    let (_d, r) = build(rv, "riscv64gc-unknown-none-elf");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("`stop.kind`"), "{}", r.stderr);
}

/// `MOVED` with `change` applied: a key added or replaced, or a table replaced whole.
fn edited(change: &str) -> String {
    if change.starts_with('[') {
        let table = change.lines().next().unwrap();
        let mut out = String::new();
        let mut skipping = false;
        for line in MOVED.lines() {
            if line.starts_with('[') {
                skipping = line == table;
                if skipping {
                    out.push_str(change);
                    continue;
                }
            }
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
        }
        if !out.contains(table) {
            out.push_str(change);
        }
        return out;
    }
    let key = change.split(' ').next().unwrap();
    let mut out: String = MOVED
        .lines()
        .filter(|l| !l.starts_with(&format!("{key} ")))
        .map(|l| format!("{l}\n"))
        .collect();
    out.insert_str(0, change);
    out
}

#[test]
fn board_needs_a_boards_target_and_a_file_that_is_there() {
    let (_d, r) = build(MOVED, "wasm32-unknown-unknown");
    assert_eq!(r.code, Some(5), "{r:?}");
    assert!(r.stderr.contains("--board"), "{}", r.stderr);
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "absent.toml",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    assert_eq!(r.code, Some(3), "{r:?}");
    // A runtime artifact is a built-in board's: with another board's manifest, refused by name.
    std::fs::write(d.path().join("board.toml"), MOVED).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--board",
            "board.toml",
            "--runtime",
            "rt",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    assert_eq!(r.code, Some(5), "{r:?}");
    assert!(r.stderr.contains("--runtime"), "{}", r.stderr);
}

/// The program built here for `MOVED`'s board, then linked with its `link.ld` and booted under
/// QEMU in the `nazm-qemu:n86` image: `7`, and the machine stopped with 0, from an image loaded
/// where the manifest says.
#[test]
#[ignore = "starts a container"]
fn an_image_for_a_manifests_board_boots_under_qemu() {
    let (d, r) = build(MOVED, "aarch64-unknown-none");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let out = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args([
            "nazm-qemu:n86",
            "sh",
            "-c",
            "cd /w/o && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf && \
             (llvm-nm x.elf || nm x.elf) | grep -q '^0000000040180000 T _start' && \
             timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none \
             -semihosting -kernel x.elf",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}
