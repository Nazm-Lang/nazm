//! Gate 3: a board's heap, and `no-heap` (`docs/spec.md`, *Boards*; `docs/systems-domains.md`
//! Part B).
//!
//! What a build shows on any host: a board whose manifest gives a heap builds a program that
//! allocates — strings, sequences, buffers — which a board without one refuses, naming what it
//! reaches and where a heap is given; the linker script bounds the arena; and the `kernel`
//! profile refuses every allocation site whatever the board. That the arena allocates and runs out
//! as the spec says is the ignored test at the end, booted under QEMU in the `nazm-qemu:n86` image.

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

/// QEMU `virt`'s AArch64 board with a heap of `size` bytes.
fn board(size: &str) -> String {
    format!(
        "name = \"virt, with a heap\"
isa = \"aarch64\"
load_address = 0x40080000

[uart]
kind = \"pl011\"
address = 0x9000000

[stop]
kind = \"semihosting\"

[heap]
size = {size}
"
    )
}

/// A thousand numbers in a `Vec`, a string built at run time, and a buffer: 499500 + 4 + 7.
const ALLOCATES: &str = "fn main(m: MmioCap) -> Int ! {} {
    let v = vec_new[Int]();
    let mut i = 0;
    while i < 1000 {
        vec_push(v, i);
        i = i + 1;
    }
    let s = str_concat(\"ab\", \"cd\");
    let b = bytes_new(16);
    bytes_set(b, 3, 7);
    let mut total = 0;
    let mut k = 0;
    while k < vec_len(v) {
        total = total + vec_get(v, k);
        k = k + 1;
    }
    total + str_len(s) + Int(bytes_get(b, 3))
}
";

/// `src` built for the AArch64 board, with `manifest` as `board.toml` where one is given.
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

#[test]
fn a_board_with_a_heap_allocates_and_one_without_refuses_by_name() {
    let (_d, r) = build(ALLOCATES, None, "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    for named in ["sequences", "strings", "`[heap]`"] {
        assert!(r.stderr.contains(named), "{named}: {}", r.stderr);
    }
    let (d, r) = build(ALLOCATES, Some(&board("0x100000")), "0");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    assert!(
        script.contains("__heap_start = .;\n  . = . + 0x100000;\n  __heap_end = .;"),
        "{script}"
    );
}

#[test]
fn the_kernel_profile_refuses_every_allocation_site_whatever_the_board() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.nz"), ALLOCATES).unwrap();
    std::fs::write(
        d.path().join("b.nz"),
        "static T: Atomic = atomic_new(0);\nfn main(m: MmioCap) -> Int ! {} { atomic_add(T, 1) }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "--profile", "kernel", "a.nz"]);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("N0510"), "{}", r.stderr);
    assert!(r.stderr.contains("`no-heap`"), "{}", r.stderr);
    for site in [
        "`vec_new` allocates",
        "`str_concat` allocates",
        "`bytes_new` allocates",
    ] {
        assert!(r.stderr.contains(site), "{site}: {}", r.stderr);
    }
    let r = nazm(d.path(), &["check", "--profile", "kernel", "b.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
}

/// Booted under QEMU in the `nazm-qemu:n86` image: the allocating program prints `499511` at
/// `-O0` and `-O2`; with a 256-byte heap it runs out, reports `N0406` and stops with 2.
#[test]
#[ignore = "starts a container"]
fn the_arena_allocates_and_runs_out_under_qemu() {
    for (opt, size, code, want) in [
        ("0", "0x100000", 0, "499511\n"),
        ("2", "0x100000", 0, "499511\n"),
        ("0", "0x100", 2, "N0406"),
    ] {
        let (d, r) = build(ALLOCATES, Some(&board(size)), opt);
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
        assert_eq!(out.status.code(), Some(code), "-O{opt} {size}: {out:?}");
        assert!(got.contains(want), "-O{opt} {size}: {got}");
    }
}
