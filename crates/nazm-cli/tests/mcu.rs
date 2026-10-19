//! M1: the microcontroller targets (`docs/user/spec.md`, *Boards*;
//! `openspec/changes/v1-every-domain/design.md`, M).
//!
//! What a build shows on any host: `thumbv8m.main-none-eabihf` (a Cortex-M33, the RP2350's core)
//! and `riscv32imc-unknown-none-elf` (an RV32IMC, the ESP32-C3's) build to objects of their own
//! architecture and float ABI, with a linker script that places code in flash and data in RAM and
//! a `_start` that copies the data and zeroes the rest before anything runs; a manifest's
//! `[memory]`, `init` and `[image]` are read, checked and refused by name; an image that does not
//! fit is `N0850`, an `init` the program does not have `N0851`, and a float the core cannot compute
//! with `N0854`. RV32 needs a `clang` with a RISC-V code generator — Apple's has none, and is
//! refused by name. That the images boot — on QEMU's `mps2-an505` (Cortex-M33) and RISC-V `virt`
//! with an RV32IMC hart (`lowrisc-ibex`) — and agree with the interpreter is the ignored test at
//! the end, run in the `nazm-qemu:n86` image. Emulated only: nothing here has run on a chip.

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

use nazm_lir::runtime::board::{self, Image, Isa, BOARDS};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");
const M33: &str = "thumbv8m.main-none-eabihf";
const RV32: &str = "riscv32imc-unknown-none-elf";

/// Each built-in board's UART data register, as a program writes it.
fn uart(target: &str) -> i64 {
    if target == M33 {
        0x5020_0000
    } else {
        0x1000_0000
    }
}

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// Whether this host's `clang` lists a 32-bit RISC-V code generator.
fn host_generates_rv32() -> bool {
    Command::new("clang")
        .arg("-print-targets")
        .output()
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.split_whitespace().next() == Some("riscv32"))
        })
}

/// `src` built for `target` into `o`, with `manifest` as `board.toml` when there is one.
fn build(target: &str, src: &str, manifest: Option<&str>, opt: &str) -> (tempfile::TempDir, Ran) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut args = vec!["build", "--opt-level", opt, "--target", target];
    if let Some(m) = manifest {
        std::fs::write(d.path().join("board.toml"), m).unwrap();
        args.extend(["--board", "board.toml"]);
    }
    args.extend(["--objects", "o", "prog.nz"]);
    let r = nazm(d.path(), &args);
    (d, r)
}

/// The names of a 32-bit little-endian ELF object's sections, and its `e_machine` and `e_flags`.
fn elf32(bytes: &[u8]) -> (u16, u32, Vec<String>) {
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    assert!(bytes.starts_with(b"\x7fELF"), "not ELF");
    assert_eq!(bytes[4], 1, "not 32-bit ELF");
    let shoff = u32_at(0x20) as usize;
    let shentsize = u16_at(0x2e) as usize;
    let shnum = u16_at(0x30) as usize;
    let strtab = u32_at(shoff + u16_at(0x32) as usize * shentsize + 16) as usize;
    let names = (0..shnum)
        .map(|i| {
            let start = strtab + u32_at(shoff + i * shentsize) as usize;
            let end = start + bytes[start..].iter().position(|c| *c == 0).unwrap();
            String::from_utf8_lossy(&bytes[start..end]).into_owned()
        })
        .collect();
    (u16_at(18), u32_at(0x24), names)
}

/// A program for `target`'s built-in board: two statics — one with a value, held in data, one
/// zero — 64-bit division and remainder, a static atomic, and recursion under the stack check.
fn arithmetic(target: &str) -> String {
    format!(
        "static HITS: Atomic = atomic_new(5);\n\
         static ZERO: Atomic = atomic_new(0);\n\
         fn put(m: MmioCap, c: Int) -> Int ! {{}} {{ mmio_write8({}, c) }}\n\
         fn fib(n: Int) -> Int {{ if n < 2 {{ n }} else {{ fib(n - 1) + fib(n - 2) }} }}\n\
         fn main(m: MmioCap) -> Int ! {{}} {{\n    \
             put(m, 48 + atomic_load(HITS));\n    \
             put(m, 48 + atomic_load(ZERO));\n    \
             atomic_add(HITS, 10);\n    \
             9223372036854775807 / 3 % 1000 + fib(10) + atomic_load(HITS)\n}}\n",
        uart(target)
    )
}

/// The compile-only evidence for both targets (`nazm_lir::backend::SUPPORT`): objects of the
/// target's architecture and float ABI, a vector table on the Cortex-M33, and a linker script with
/// the board's flash and RAM, data loaded in flash and run in RAM, and the assertions that name
/// `N0850`. RV32 where the host's `clang` can generate it, and refused by name where it cannot.
#[test]
fn an_image_for_each_microcontroller_target_is_laid_out_for_its_memory() {
    for target in [M33, RV32] {
        let (d, r) = build(target, &arithmetic(target), None, "0");
        if target == RV32 && !host_generates_rv32() {
            assert_eq!(r.code, Some(4), "{}", r.stderr);
            assert!(
                r.stderr.contains("has no RISC-V code generator"),
                "{}",
                r.stderr
            );
            continue;
        }
        assert_eq!(r.code, Some(0), "{target}: {}", r.stderr);
        let b = board::board(target).unwrap();
        let m = b.memory.unwrap();
        let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
        for want in [
            format!(
                "FLASH (rx) : ORIGIN = {:#x}, LENGTH = {:#x}",
                m.flash.origin, m.flash.length
            ),
            format!(
                "RAM (rwx) : ORIGIN = {:#x}, LENGTH = {:#x}",
                m.ram.origin, m.ram.length
            ),
            "> RAM AT > FLASH".to_owned(),
            "__data_load = LOADADDR(.data);".to_owned(),
            ".bss (NOLOAD)".to_owned(),
            "nazm: error[N0850]".to_owned(),
        ] {
            assert!(script.contains(&want), "{target}: {want}\n{script}");
        }
        assert_eq!(
            script.contains("KEEP(*(.vectors)) *(.text.start)"),
            target == M33,
            "{script}"
        );
        let link = std::fs::read_to_string(d.path().join("o/link.txt")).unwrap();
        assert!(link.contains(&format!("target {target}\n")), "{link}");
        let (machine, flags, sections) = elf32(&std::fs::read(d.path().join("o/00-m0.o")).unwrap());
        if target == M33 {
            // EM_ARM, EABI version 5; the float ABI is in its build attributes.
            assert_eq!(machine, 40);
            assert_eq!(flags & 0xff00_0000, 0x0500_0000, "{flags:#x}");
            assert!(
                sections.iter().any(|s| s == ".ARM.attributes"),
                "{sections:?}"
            );
        } else {
            // EM_RISCV; compressed instructions, the soft-float ABI, no RVE.
            assert_eq!(machine, 243);
            assert_eq!(flags & 0x1, 0x1, "{flags:#x}");
            assert_eq!(flags & 0x6, 0, "{flags:#x}");
            assert_eq!(flags & 0x8, 0, "{flags:#x}");
        }
        let entry = std::fs::read(d.path().join("o/02-entry.o")).unwrap();
        let (_, _, sections) = elf32(&entry);
        assert!(sections.iter().any(|s| s == ".text.start"), "{sections:?}");
        assert_eq!(
            sections.iter().any(|s| s == ".vectors"),
            target == M33,
            "{sections:?}"
        );
    }
}

/// What `_start` does on each, from the board's description: the stack; on the Cortex-M33 its FPU
/// opened in CPACR, on RV32 a trap vector; then the data copied from flash and the zeroed data
/// zeroed — before the stack is painted and before `main` — and the 64-bit operations each core
/// has no instruction for defined by the board's runtime.
#[test]
fn the_startup_copies_data_and_zeroes_the_rest_before_anything_runs() {
    for target in [M33, RV32] {
        let b = board::board(target).unwrap();
        let entry = board::emit_entry(b, "@main", 0, true);
        let at = |s: &str| {
            entry
                .find(s)
                .unwrap_or_else(|| panic!("{target}: {s}\n{entry}"))
        };
        let (copy, zero, paint) = if target == M33 {
            assert!(at("ldr r0, =0xE000ED88") < at("ldr r0, =__data_start"));
            assert!(entry.contains("orr r1, r1, #0xF00000"), "{entry}");
            (
                at("ldr r3, [r2], #4"),
                at("ldr r0, =__bss_start"),
                at("ldr r2, =0x5a5a5a5a"),
            )
        } else {
            assert!(at("csrw mtvec, t0") < at("la t0, __data_start"));
            assert!(entry.contains("nz_fault:"), "{entry}");
            (
                at("lw t3, 0(t2)"),
                at("la t0, __bss_start"),
                at("li t2, 0x5a5a5a5a"),
            )
        };
        let main = if target == M33 {
            at("bl nz_board_main")
        } else {
            at("call nz_board_main")
        };
        assert!(copy < zero && zero < paint && paint < main, "{entry}");
        let rt = board::emit_runtime(b, true);
        let helpers: &[&str] = if target == M33 {
            &[
                "@__atomic_compare_exchange_8(",
                "@__aeabi_ldivmod(",
                "@__aeabi_l2f(",
                "@__aeabi_f2lz(",
                "@__aeabi_f2d(",
                "@__aeabi_dcmpge(",
            ]
        } else {
            &[
                "@__atomic_compare_exchange_8(",
                "@__divdi3(",
                "@__umoddi3(",
                "@memcpy(ptr %d, ptr %s, i32 %n)",
                "csrrci $0, mstatus, 8",
            ]
        };
        for h in helpers {
            assert!(rt.contains(h), "{target}: {h}");
        }
        // The inlined stack check's limit, which no board's runtime defined before M1.
        assert!(rt.contains("@nz.stack_limit = hidden global i64 0"), "{rt}");
        assert!(!rt.contains("@memcpy(ptr %d, ptr %s, i64 %n)"), "{target}");
    }
    // A board loaded whole has none of it.
    for b in BOARDS.iter().filter(|b| b.memory.is_none()) {
        assert!(!board::emit_entry(b, "@main", 0, false).contains("__data_load"));
        assert!(!board::linker_script(b).contains("MEMORY"));
    }
    // Both new boards are microcontrollers that run from flash.
    for t in [M33, RV32] {
        let b = board::board(t).unwrap();
        assert!(b.isa.microcontroller() && b.memory.is_some() && !b.hardware);
    }
    assert_eq!(board::board(M33).unwrap().isa, Isa::Thumbv8m);
    assert_eq!(board::board(RV32).unwrap().isa, Isa::Riscv32);
}

/// The Cortex-M33 board with an `init` and the RP2350's image block.
const M33_BOARD: &str = "name = \"AN505, with an init\"
isa = \"thumbv8m.main\"
init = \"board_start\"

[uart]
kind = \"cmsdk\"
address = 0x50200000

[stop]
kind = \"semihosting\"

[memory]
flash = { origin = 0x10000000, length = 0x400000 }
ram = { origin = 0x38000000, length = 0x200000 }

[image]
format = \"rp2350\"
";

/// The RV32 board with an `init`.
const RV32_BOARD: &str = "name = \"RV32 virt, with an init\"
isa = \"riscv32\"
init = \"board_start\"

[uart]
kind = \"ns16550a\"
address = 0x10000000

[stop]
kind = \"sifive-test\"
address = 0x100000

[memory]
flash = { origin = 0x80000000, length = 0x100000 }
ram = { origin = 0x80100000, length = 0x100000 }
";

/// `init` runs after the data is copied — it sees the static's 7 — and before `main`, which sees
/// what it stored.
fn with_init(target: &str) -> String {
    format!(
        "static READY: Atomic = atomic_new(7);\n\
         fn put(m: MmioCap, c: Int) -> Int ! {{}} {{ mmio_write8({}, c) }}\n\
         fn board_start(m: MmioCap) -> Int ! {{}} {{\n    \
             put(m, 48 + atomic_load(READY));\n    put(m, 10);\n    atomic_store(READY, 1)\n}}\n\
         fn main(m: MmioCap) -> Int ! {{}} {{\n    put(m, 77);\n    put(m, 10);\n    \
             atomic_load(READY) + 40\n}}\n",
        uart(target)
    )
}

#[test]
fn a_manifest_gives_a_microcontroller_flash_ram_an_init_and_an_image() {
    let (d, r) = build(M33, &with_init(M33), Some(M33_BOARD), "0");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let script = std::fs::read_to_string(d.path().join("o/link.ld")).unwrap();
    // The RP2350's block, after the vector table and so in the first 4 KiB of flash.
    assert!(
        script.contains("KEEP(*(.vectors)) KEEP(*(.image_def)) *(.text.start)"),
        "{script}"
    );
    let entry = std::fs::read(d.path().join("o/02-entry.o")).unwrap();
    let (_, _, sections) = elf32(&entry);
    assert!(sections.iter().any(|s| s == ".image_def"), "{sections:?}");
    let words: Vec<u8> = board::RP2350_IMAGE_DEF
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert!(
        entry.windows(words.len()).any(|w| w == words.as_slice()),
        "the image block's words"
    );
    // `_start` enters the board's `init`, which the entry defines around the program's function.
    assert!(entry.windows(13).any(|w| w == b"nz_board_init"));
    // Each key read back, by the built-in boards' own manifests too.
    for b in &BOARDS {
        let text = b.manifest();
        if b.memory.is_some() {
            assert!(text.contains("[memory]\nflash = { origin = "), "{text}");
        }
    }
    let read = |text: &str| {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), with_init(M33)).unwrap();
        std::fs::write(d.path().join("board.toml"), text).unwrap();
        nazm(
            d.path(),
            &[
                "build",
                "--target",
                M33,
                "--board",
                "board.toml",
                "--objects",
                "o",
                "prog.nz",
            ],
        )
    };
    assert_eq!(Image::Rp2350.name(), "rp2350");
    // Each refused by name (`N0626`).
    for (from, to, key) in [
        ("isa = \"thumbv8m.main\"", "isa = \"thumbv7m\"", "`isa`"),
        (
            "isa = \"thumbv8m.main\"",
            "isa = \"thumbv8m.main\"\nload_address = 0x0",
            "`load_address`",
        ),
        (
            "ram = { origin = 0x38000000",
            "ram = { origin = 0x10000000",
            "`memory.ram`",
        ),
        ("length = 0x200000 }", "length = 0x200002 }", "`memory.ram`"),
        (
            "flash = { origin = 0x10000000, length = 0x400000 }",
            "flash = { origin = 0xfffff000, length = 0x2000 }",
            "`memory.flash`",
        ),
        (
            "length = 0x400000 }",
            "length = 0x400000, end = 1 }",
            "`memory.flash.end`",
        ),
        ("format = \"rp2350\"", "format = \"uf2\"", "`image.format`"),
        ("init = \"board_start\"", "init = \"board start\"", "`init`"),
        ("[image]", "[heap]\nsize = 4096\n\n[image]", "`heap`"),
    ] {
        let r = read(&M33_BOARD.replace(from, to));
        assert_eq!(r.code, Some(1), "{to}: {r:?}");
        assert!(r.stderr.contains("N0626"), "{to}: {}", r.stderr);
        assert!(r.stderr.contains(key), "{to}: {}", r.stderr);
    }
    // The RP2350's image needs flash.
    let flashless = M33_BOARD
        .replace(
            "[memory]\nflash = { origin = 0x10000000, length = 0x400000 }\nram = { origin = 0x38000000, length = 0x200000 }\n",
            "",
        )
        .replace("init = \"board_start\"", "init = \"board_start\"\nload_address = 0x10000000");
    let r = read(&flashless);
    assert!(
        r.code == Some(1) && r.stderr.contains("N0626") && r.stderr.contains("`image.format`"),
        "{r:?}"
    );
    // A 64-bit board is loaded whole: no `[memory]`.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main(m: MmioCap) -> Int ! {} { 0 }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("board.toml"),
        "name = \"virt\"\nisa = \"aarch64\"\nload_address = 0x40080000\n\n[uart]\nkind = \"pl011\"\naddress = 0x9000000\n\n[stop]\nkind = \"semihosting\"\n\n[memory]\nflash = { origin = 0x0, length = 0x1000 }\nram = { origin = 0x40000000, length = 0x100000 }\n",
    )
    .unwrap();
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
            "prog.nz",
        ],
    );
    assert!(
        r.code == Some(1) && r.stderr.contains("N0626") && r.stderr.contains("`memory`"),
        "{r:?}"
    );
    // A RISC-V microcontroller's `[interrupts]` waits for its controller (M3), as RV64's does.
    if host_generates_rv32() {
        let (_, r) = build(
            RV32,
            &with_init(RV32),
            Some(&format!(
                "{RV32_BOARD}\n[interrupts]\ncontroller = \"nvic\"\nclock_hz = 1\n"
            )),
            "0",
        );
        assert!(
            r.code == Some(1) && r.stderr.contains("`interrupts`"),
            "{r:?}"
        );
    }
}

#[test]
fn an_image_that_does_not_fit_the_boards_memory_is_refused_naming_the_region() {
    // The stack alone, at the manifest.
    let small_ram = M33_BOARD.replace("length = 0x200000 }", "length = 0x8000 }");
    let (d, r) = build(M33, &with_init(M33), Some(&small_ram), "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("error[N0850]"), "{}", r.stderr);
    assert!(
        r.stderr.contains("`stack`")
            && r.stderr.contains("RAM")
            && r.stderr.contains("over by 0x8000"),
        "{}",
        r.stderr
    );
    assert!(!d.path().join("o").exists(), "nothing is written");
    // The objects, at the build: the code is larger than a 1 KiB flash.
    let small_flash = M33_BOARD.replace("length = 0x400000 }", "length = 0x400 }");
    let (d, r) = build(M33, &with_init(M33), Some(&small_flash), "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(r.stderr.contains("error[N0850]"), "{}", r.stderr);
    assert!(
        r.stderr.contains("flash") && r.stderr.contains("over by"),
        "{}",
        r.stderr
    );
    assert!(!d.path().join("o").exists(), "nothing is written");
    // The data and the stack together, at the build: RAM one byte-word short of both.
    let tight = M33_BOARD.replace("length = 0x200000 }", "length = 0x10000 }");
    let (_d, r) = build(M33, &with_init(M33), Some(&tight), "0");
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(
        r.stderr.contains("error[N0850]") && r.stderr.contains("RAM"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_boards_init_is_one_function_of_the_program_of_the_boards_shape() {
    for (src, says) in [
        (
            with_init(M33).replace("fn board_start(", "fn board_begin("),
            "has no function of that name",
        ),
        (
            with_init(M33).replace(
                "fn board_start(m: MmioCap) -> Int ! {}",
                "fn board_start(m: MmioCap, n: Int) -> Int ! {}",
            ),
            "is not `fn board_start() -> Int ! {}`",
        ),
    ] {
        let (_d, r) = build(M33, &src, Some(M33_BOARD), "0");
        assert_eq!(r.code, Some(1), "{src}: {r:?}");
        assert!(r.stderr.contains("N0851"), "{}", r.stderr);
        assert!(r.stderr.contains(says), "{}", r.stderr);
    }
    // Without the device's authority, too.
    let src = with_init(M33)
        .replace(
            "fn board_start(m: MmioCap) -> Int ! {}",
            "fn board_start() -> Int ! {}",
        )
        .replace(
            "    put(m, 48 + atomic_load(READY));\n    put(m, 10);\n",
            "",
        );
    let (d, r) = build(M33, &src, Some(M33_BOARD), "2");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let entry = std::fs::read(d.path().join("o/02-entry.o")).unwrap();
    assert!(entry.windows(13).any(|w| w == b"nz_board_init"));
}

#[test]
fn a_float_a_microcontroller_cannot_compute_with_is_refused_by_name() {
    // The Cortex-M33's FPU is single-precision: `Float32` builds, `Float64` is refused wherever it
    // is held — a parameter, a binding, or a temporary of a conversion.
    let f32 = "fn half(x: Float32) -> Float32 { x / Float32(2) }\n\
               fn main(m: MmioCap) -> Int ! {} { Int(half(Float32(9))) }\n";
    let (_d, r) = build(M33, f32, None, "0");
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    for src in [
        "fn half(x: Float64) -> Float64 { x / 2.0 }\nfn main(m: MmioCap) -> Int ! {} { 0 }\n",
        "fn main(m: MmioCap) -> Int ! {} { let x = 1.5; Int(x * 2.0) }\n",
        "fn main(m: MmioCap) -> Int ! {} { Int(Float64(Float32(1.5)) * 2.0) }\n",
    ] {
        let (_d, r) = build(M33, src, None, "0");
        assert_eq!(r.code, Some(1), "{src}: {r:?}");
        assert!(r.stderr.contains("error[N0854]"), "{src}: {}", r.stderr);
        assert!(r.stderr.contains("`Float64`"), "{src}: {}", r.stderr);
    }
    // An RV32IMC has no FPU: both are refused, before any code generator is asked.
    if host_generates_rv32() {
        for src in [
            f32,
            "fn main(m: MmioCap) -> Int ! {} { let x = 2.5; Int(x) }\n",
        ] {
            let (_d, r) = build(RV32, src, None, "0");
            assert_eq!(r.code, Some(1), "{src}: {r:?}");
            assert!(r.stderr.contains("error[N0854]"), "{src}: {}", r.stderr);
        }
    }
    // And a C pointer is refused on both, as on every 32-bit target (Q1-A1-05).
    let src = "extern \"C\" struct Counter;\n\
               extern \"C\" fn h_use(c: Counter, n: Int) -> Int = \"h_use\";\n\
               fn main(f: ForeignCap) -> Int ! { foreign } { 0 }\n";
    let (_d, r) = build(M33, src, None, "0");
    assert!(r.stderr.contains("error[N0381]"), "{r:?}");
}

/// The programs the emulator runs, each twice — on the board, and in the interpreter as its
/// oracle: `show` writes a number in decimal on the UART, or prints it.
const SHOW: &str = "fn show(m: MmioCap, n: Int) -> Int ! {} {
    let mut v = n;
    if v < 0 { put(m, 45); v = 0 - v; }
    let mut d = 1000000000000000000;
    let mut started = false;
    while d > 0 {
        let q = v / d;
        if q > 0 || started || d == 1 { put(m, 48 + q); started = true; }
        v = v % d;
        d = d / 10;
    }
    put(m, 10)
}
";

/// 64-bit integers on a 32-bit core: data and zeroed statics, division and remainder, a checked
/// product, bit counts, unsigned division, recursion, a record, an array, and atomics.
const INTS: &str = "static HITS: Atomic = atomic_new(5);
static ZERO: Atomic = atomic_new(0);
static TABLE: UInt32 = 3735928559;

struct Pair { a: Int, b: Int, }

fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }

fn sum(xs: [Int; 8]) -> Int {
    let mut t = 0;
    let mut i = 0;
    while i < 8 { t = t + xs[i]; i = i + 1; }
    t
}

fn main(m: MmioCap) -> Int ! {} {
    show(m, atomic_load(HITS));
    show(m, atomic_load(ZERO));
    show(m, Int(TABLE));
    show(m, -7 / 2);
    show(m, -7 % 2);
    show(m, 9223372036854775807 / 3);
    show(m, -9223372036854775807 % 1000000007);
    show(m, 3037000499 * 3037000499);
    show(m, Int(count_ones(TABLE)));
    show(m, Int(leading_zeros(12345)));
    show(m, Int(UInt64(18446744073709551615) / UInt64(10)));
    show(m, wrap_convert[Int](UInt64(18446744073709551615) % UInt64(1000)));
    show(m, fib(20));
    let p = Pair(a: 40, b: 2);
    show(m, p.a + p.b);
    let xs: [Int; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    show(m, sum(xs));
    atomic_add(HITS, 10);
    show(m, atomic_swap(HITS, 1));
    show(m, if atomic_compare_swap(HITS, 1, 2) { atomic_load(HITS) } else { 0 });
    atomic_load(HITS) + 40
}
";

/// `Float32` on the Cortex-M33's FPU: arithmetic, conversions to and from 64-bit integers (the
/// runtime's helpers), the exact range check of a conversion, and the rounding functions it has
/// instructions for.
const FLOATS: &str = "fn bits(x: Float32) -> Int { Int(float32_to_bits(x)) }

fn main(m: MmioCap) -> Int ! {} {
    let x: Float32 = 1.5;
    let y = x * Float32(3);
    show(m, Int(y * Float32(2)));
    show(m, bits(y / Float32(7)));
    show(m, bits(Float32(9007199254740993)));
    show(m, bits(Float32(-9223372036854775807)));
    show(m, bits(Float32(UInt64(18446744073709551615))));
    show(m, Int(Float32(-1234567.75)));
    show(m, wrap_convert[Int](UInt64(Float32(1.5e19))));
    show(m, bits(float_sqrt(y)));
    show(m, bits(float_floor(Float32(-2.5))));
    show(m, bits(float_ceil(Float32(-2.5))));
    show(m, bits(float_trunc(Float32(-2.5))));
    show(m, bits(float_abs(Float32(-3))));
    show(m, saturate_convert[Int](Float32(1e30)));
    show(m, saturate_convert[Int](Float32(-1e30)) + 1);
    show(m, match try_convert[Int](Float32(2.5e18)) { Some(v) => v, None => 0 });
    if float_is_nan(Float32(0) / Float32(0)) { show(m, 1); } else { show(m, 0); }
    0
}
";

/// A program for the board: `put` at its UART, `show`, and `body`.
fn on_board(target: &str, body: &str) -> String {
    format!(
        "fn put(m: MmioCap, c: Int) -> Int ! {{}} {{ mmio_write8({}, c) }}\n{SHOW}\n{body}",
        uart(target)
    )
}

/// The same program for the interpreter: `show` prints, and `main`'s result is printed after, as
/// a board writes it.
fn in_interpreter(body: &str) -> String {
    format!(
        "fn show(m: Int, n: Int) -> Int {{ println(num_to_str(n)); 0 }}\n{}",
        body.replace(
            "fn main(m: MmioCap) -> Int ! {} {",
            "fn main() -> Int { println(num_to_str(run(0))); 0 }\nfn run(m: Int) -> Int {"
        )
    )
}

/// Builds `nazm` from a copy of this tree inside the `nazm-qemu:n86` image — whose `clang` has
/// both code generators — then builds, links and boots each program at `-O0` and `-O2` on QEMU's
/// `mps2-an505` (a Cortex-M33) and RISC-V `virt` with an RV32IMC hart (`-cpu lowrisc-ibex`, so an
/// atomic or floating-point instruction would trap): the integer program on both and the
/// `Float32` program on the Cortex-M33 agree with the interpreter; recursion meets `N0408` at the
/// board's stack; and a manifest's `init` runs after the data is copied and before `main`, with the
/// RP2350's image block in place. Needs Docker and the image:
/// `cargo test -p nazm-cli --test cli_m_r -- --ignored mcu::`.
#[test]
#[ignore = "starts a container"]
fn the_microcontroller_images_boot_and_agree_under_qemu() {
    let ev = tempfile::tempdir().unwrap();
    let w = |name: &str, text: &str| std::fs::write(ev.path().join(name), text).unwrap();
    for t in [M33, RV32] {
        w(&format!("ints-{t}.nz"), &on_board(t, INTS));
        w(
            &format!("deep-{t}.nz"),
            &format!(
                "fn put(m: MmioCap, c: Int) -> Int ! {{}} {{ mmio_write8({}, c) }}\n\
             fn down(n: Int) -> Int {{ if n == 0 {{ 0 }} else {{ 1 + down(n - 1) }} }}\n\
             fn main(m: MmioCap) -> Int ! {{}} {{ put(m, 68); put(m, 10); down(100000000) }}\n",
                uart(t)
            ),
        );
        w(&format!("init-{t}.nz"), &with_init(t));
    }
    w(&format!("floats-{M33}.nz"), &on_board(M33, FLOATS));
    w(&format!("board-{M33}.toml"), M33_BOARD);
    w(&format!("board-{RV32}.toml"), RV32_BOARD);
    w("ints-host.nz", &in_interpreter(INTS));
    w("floats-host.nz", &in_interpreter(FLOATS));
    let script = format!(
        r#"set -eu
mkdir -p /work/m1
cd /src && tar --exclude=./target --exclude=./.git -cf - . | (cd /work/m1 && tar xf -)
cd /work/m1 && CARGO_TARGET_DIR=/work/target cargo build -q -p nazm-cli --offline
N=/work/target/debug/nazm
cd /ev
for p in ints floats; do
  $N run --interpret $p-host.nz > /tmp/oracle-$p.txt
  printf 'oracle %s %s\n' "$p" "$(tr '\n' '|' < /tmp/oracle-$p.txt)"
done
for t in {M33} {RV32}; do
  case $t in
    thumb*) q="qemu-system-arm -M mps2-an505 -cpu cortex-m33 -nographic -net none -semihosting -kernel"; ps="ints floats deep init";;
    *) q="qemu-system-riscv32 -M virt -bios none -cpu lowrisc-ibex -nographic -nic none -kernel"; ps="ints deep init";;
  esac
  for o in 0 2; do
    for p in $ps; do
      d=/tmp/o-$t-$p-$o
      b=""
      if [ $p = init ]; then b="--board board-$t.toml"; fi
      $N build --opt-level $o --target $t $b --objects $d $p-$t.nz > /dev/null
      (cd $d && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf)
      set +e; out=$(timeout 20 $q $d/x.elf 2>/dev/null); code=$?; set -e
      printf '%s %s %s %s %s\n' "$t" "$p" "$o" "$code" "$(printf '%s\n' "$out" | tr '\n' '|')"
    done
  done
done
"#
    );
    w("run.sh", &script);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "--memory",
            "8g",
            "--memory-swap",
            "8g",
            "--cpus",
            "4",
            "--pids-limit",
            "4096",
            "-v",
        ])
        .arg(format!(
            "{}:/src:ro",
            repo.canonicalize().unwrap().display()
        ))
        .arg("-v")
        .arg(format!("{}:/ev", ev.path().display()))
        .args(["nazm-qemu:n86", "sh", "/ev/run.sh"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    let out = String::from_utf8_lossy(&o.stdout);
    let field = |prefix: &str| -> String {
        out.lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("{prefix}: {out}"))
            .to_owned()
    };
    for p in ["ints", "floats"] {
        let oracle = field(&format!("oracle {p} "));
        assert!(oracle.contains('|'), "{p}: {oracle}");
        for t in [M33, RV32] {
            if p == "floats" && t == RV32 {
                continue;
            }
            for opt in ["0", "2"] {
                // Status 0, and every line the interpreter's.
                assert_eq!(
                    field(&format!("{t} {p} {opt} ")),
                    format!("0 {oracle}"),
                    "{t} {p} -O{opt}"
                );
            }
        }
    }
    for t in [M33, RV32] {
        for opt in ["0", "2"] {
            let deep = field(&format!("{t} deep {opt} "));
            assert!(
                deep.starts_with("2 D|nazm: error[N0408]"),
                "{t} -O{opt}: {deep}"
            );
            assert_eq!(
                field(&format!("{t} init {opt} ")),
                "0 7|M|41|",
                "{t} -O{opt}"
            );
        }
    }
}
