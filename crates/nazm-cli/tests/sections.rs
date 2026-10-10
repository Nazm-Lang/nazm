//! Gate 3: `@section` and the 64-bit device registers (`docs/spec.md`, *Boards*;
//! `docs/systems-domains.md` Part B).
//!
//! What a board build shows on any host: a function and a static `@section` names reach the
//! object in that section, and the generated `link.ld` keeps each beside the board's own sections
//! of its kind; everything else — a hosted build, `nazm run`, an attribute on what has no address
//! — is refused by name. That the image links and boots is the ignored test at the end, run by
//! hand against the `nazm-qemu:n86` image: it builds here and only links and boots there.

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

/// A function, a writable static and a read-only static, each in a section of its own, and a
/// 64-bit device register written and read.
const PLACED: &str = "@section(\".data.ticks\")
static TICKS: Atomic = atomic_new(40);

@section(\".rodata.table\")
static BASE: Int = 2;

@section(\".boot\")
fn bump() -> Int { atomic_add(TICKS, BASE) }

fn main(m: MmioCap) -> Int ! {} {
    bump();
    atomic_load(TICKS)
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
fn a_placed_definition_reaches_its_section_and_the_linker_script_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PLACED).unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--objects",
            "o",
            "prog.nz",
        ],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    assert!(
        script.contains(".text : { *(.text.start) *(.text*) KEEP(*(.boot)) }"),
        "{script}"
    );
    assert!(
        script.contains(".rodata : { *(.rodata*) *(.srodata*) KEEP(*(.rodata.table)) }"),
        "{script}"
    );
    assert!(
        script.contains(".data : { *(.data*) *(.sdata*) KEEP(*(.data.ticks)) }"),
        "{script}"
    );
    // The section names are in the module's object: its section header string table.
    let object = std::fs::read(d.path().join("o/00-m0.o")).unwrap();
    for name in [".boot", ".data.ticks", ".rodata.table"] {
        assert!(
            object
                .windows(name.len() + 1)
                .any(|w| w[..name.len()] == *name.as_bytes() && w[name.len()] == 0),
            "{name} is not a section of the object"
        );
    }
    // A program without a section gets the board's script exactly as before.
    std::fs::write(
        d.path().join("plain.nz"),
        "fn main(m: MmioCap) -> Int ! {} { 0 }\n",
    )
    .unwrap();
    let r = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--objects",
            "p",
            "plain.nz",
        ],
    );
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let plain = std::fs::read_to_string(d.path().join("p/link.ld")).unwrap();
    assert!(!plain.contains("KEEP"), "{plain}");
}

#[test]
fn a_section_and_a_64_bit_register_are_a_boards_alone() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PLACED).unwrap();
    let built = nazm(d.path(), &["build", "prog.nz", "-o", "x"]);
    assert_eq!(built.code, Some(1), "{built:?}");
    assert!(built.stderr.contains("N0392"), "{}", built.stderr);
    assert!(
        built.stderr.contains("`@section` on `bump`"),
        "{}",
        built.stderr
    );
    let ran = nazm(d.path(), &["run", "prog.nz"]);
    assert_eq!(ran.code, Some(1), "{ran:?}");
    assert!(ran.stderr.contains("N0392"), "{}", ran.stderr);
    let wasm = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--objects",
            "w",
            "prog.nz",
        ],
    );
    assert!(wasm.stderr.contains("N0392"), "{}", wasm.stderr);
    std::fs::write(
        d.path().join("reg.nz"),
        "fn main(m: MmioCap) -> Int ! {} { mmio_write64(150994944, 1); mmio_read64(150994944) }\n",
    )
    .unwrap();
    let hosted = nazm(d.path(), &["run", "reg.nz"]);
    assert!(hosted.stderr.contains("N0392"), "{}", hosted.stderr);
    assert!(hosted.stderr.contains("mmio_write64"), "{}", hosted.stderr);
    let board = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "aarch64-unknown-none",
            "--objects",
            "r",
            "reg.nz",
        ],
    );
    assert_eq!(board.code, Some(0), "{}", board.stderr);
}

#[test]
fn a_section_marks_a_function_or_a_static_by_one_plain_name() {
    const MAIN: &str = "fn main() -> Int { 0 }\n";
    for src in [
        // A constant has no address; a type is not placed.
        "@section(\".x\")\nconst A: Int = 1;\n",
        "@section(\".x\")\nstruct P { x: Int, }\n",
        // One string, of letters, digits, `.` and `_`.
        "@section\nfn f() -> Int { 0 }\n",
        "@section(\"\")\nfn f() -> Int { 0 }\n",
        "@section(\"a b\")\nfn f() -> Int { 0 }\n",
        "@section(\".a\", \".b\")\nfn f() -> Int { 0 }\n",
        "@section(x)\nfn f() -> Int { 0 }\n",
        // Once.
        "@section(\".a\")\n@section(\".b\")\nfn f() -> Int { 0 }\n",
    ] {
        assert_eq!(refused(&format!("{src}{MAIN}")), ["N0619"], "{src}");
    }
}

/// Builds the placed program for the AArch64 board here, then links it with its `link.ld` and
/// boots it under QEMU in the `nazm-qemu:n86` image (`docker/qemu.Dockerfile`), at `-O0` and
/// `-O2`: it prints `42` — the static cell's 40 and the read-only static's 2 — and stops with 0.
/// A 64-bit register access, aimed at RAM, writes and reads back every bit of
/// `0x0123456789ABCDEF`, which a narrower access would cut.
#[test]
#[ignore = "starts a container"]
fn a_placed_image_links_and_boots_under_qemu() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PLACED).unwrap();
    // 0x44000000 is RAM on QEMU's `virt`, above the image and its stack.
    std::fs::write(
        d.path().join("wide.nz"),
        "fn main(m: MmioCap) -> Int ! {} { mmio_write64(1140850688, 81985529216486895); \
         mmio_read64(1140850688) }\n",
    )
    .unwrap();
    for (o, prog, want) in [
        ("0", "prog", "42\n"),
        ("2", "prog", "42\n"),
        ("0", "wide", "81985529216486895\n"),
        ("2", "wide", "81985529216486895\n"),
    ] {
        let dir = format!("{prog}{o}");
        let r = nazm(
            d.path(),
            &[
                "build",
                "--opt-level",
                o,
                "--target",
                "aarch64-unknown-none",
                "--objects",
                &dir,
                &format!("{prog}.nz"),
            ],
        );
        assert_eq!(r.code, Some(0), "{}", r.stderr);
        let script = format!(
            "cd /w/{dir} && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf && \
             timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none \
             -semihosting -kernel x.elf"
        );
        let out = Command::new("docker")
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{}:/w", d.path().display()))
            .args(["nazm-qemu:n86", "sh", "-c", &script])
            .output()
            .unwrap();
        assert!(out.status.success(), "{dir}: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{dir}");
    }
}
