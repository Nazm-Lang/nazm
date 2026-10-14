//! N62: a freestanding target — `aarch64-unknown-none`, no OS, no libc, no heap.
//!
//! `docs/architecture.md` §7.64. What a whole build shows: a program for the board is built to
//! three objects, `link.txt` and `link.ld`; the board's runtime has no thread-local storage and
//! calls nothing of a C library; a program that reaches allocation, input and output or tasks is
//! refused by naming what it reaches; a device register is refused by `nazm run` and by a hosted
//! build (`N0392`) and needs an `MmioCap` held; and Cranelift is refused for the board by name.
//! That the image boots on QEMU's `virt` machine is the ignored test at the end, run by hand in
//! the `nazm-qemu:n62` container — a test does not start a container by itself.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const BOARD: &str = "aarch64-unknown-none";

/// Writes `Hi` through the PL011 UART's data register and returns 45 + fib(10) = 100.
const PROGRAM: &str = "fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write32(150994944, c) }\n\
                       fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
                       fn main(m: MmioCap) -> Int ! {} {\n\
                       \x20   put(m, 72);\n\
                       \x20   put(m, 105);\n\
                       \x20   put(m, 10);\n\
                       \x20   let mut i = 0;\n\
                       \x20   let mut s = 0;\n\
                       \x20   while i < 10 { s = s + i; i = i + 1; }\n\
                       \x20   s + fib(10)\n\
                       }\n";

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn program(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    d
}

#[test]
fn a_board_program_is_built_to_objects_a_link_list_and_a_linker_script() {
    let d = program(PROGRAM);
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert!(o.status.success(), "{}", err(&o));
    let out = d.path().join("out");
    for name in ["00-m0.o", "01-runtime.o", "02-entry.o"] {
        let b = std::fs::read(out.join(name)).unwrap();
        assert!(b.starts_with(b"\x7fELF"), "{name} is not ELF");
        // EM_AARCH64.
        assert_eq!(u16::from_le_bytes([b[18], b[19]]), 183, "{name}");
    }
    let list = std::fs::read_to_string(out.join("link.txt")).unwrap();
    assert!(list.contains("target aarch64-unknown-none"), "{list}");
    assert!(
        list.contains("objects 00-m0.o 01-runtime.o 02-entry.o"),
        "{list}"
    );
    assert!(list.contains("ld.lld -T link.ld"), "{list}");
    let script = std::fs::read_to_string(out.join("link.ld")).unwrap();
    assert!(script.contains("ENTRY(_start)"), "{script}");
    assert!(script.contains(". = 0x40080000;"), "{script}");
    assert!(script.contains("__stack_bottom = .;"), "{script}");
    assert!(script.contains("__stack_top = .;"), "{script}");
}

#[test]
fn a_hosted_target_writes_no_linker_script() {
    let d = program("fn main() -> Int { 0 }\n");
    let o = nazm(
        d.path(),
        &[
            "build",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert!(!d.path().join("out/link.ld").exists());
}

#[test]
fn the_board_runtime_has_no_thread_local_storage_and_no_c_library() {
    use nazm_lir::runtime::board;
    for stack in [false, true] {
        let rt = board::emit_runtime(board::AARCH64, stack);
        assert!(!rt.contains("thread_local"), "{rt}");
        for libc in [
            "@malloc",
            "@free",
            "@write",
            "@exit",
            "@pthread",
            "@getrlimit",
        ] {
            assert!(!rt.contains(libc), "the board runtime calls {libc}");
        }
        assert_eq!(rt.contains("@nz.stack_check"), stack);
        assert!(rt.contains("load volatile i32"), "{rt}");
        assert!(rt.contains("store volatile i32"), "{rt}");
    }
    let entry = board::emit_entry(board::AARCH64, "@nz.m.m0.main", 1, false);
    assert!(entry.contains("_start:"), "{entry}");
    assert!(entry.contains("ldr x0, =__stack_top"), "{entry}");
    assert!(entry.contains("msr cpacr_el1"), "{entry}");
    assert!(entry.contains("call i64 @nz.m.m0.main(i64 0)"), "{entry}");
}

/// How many symbols of an ELF64 object are thread-local (`STT_TLS`), from its symbol tables.
fn tls_symbols(path: &Path) -> usize {
    let b = std::fs::read(path).unwrap();
    let u16_at = |o: usize| usize::from(u16::from_le_bytes([b[o], b[o + 1]]));
    let u64_at =
        |o: usize| usize::try_from(u64::from_le_bytes(b[o..o + 8].try_into().unwrap())).unwrap();
    let (shoff, shentsize, shnum) = (u64_at(0x28), u16_at(0x3a), u16_at(0x3c));
    let mut n = 0;
    for k in 0..shnum {
        let sh = shoff + k * shentsize;
        // SHT_SYMTAB.
        if u32::from_le_bytes(b[sh + 4..sh + 8].try_into().unwrap()) != 2 {
            continue;
        }
        let (off, size) = (u64_at(sh + 0x18), u64_at(sh + 0x20));
        n += (off..off + size)
            .step_by(24)
            .filter(|e| b[e + 4] & 0xf == 6)
            .count();
    }
    n
}

#[test]
fn a_unit_for_the_board_refers_to_no_thread_local_storage() {
    // An addition that can overflow records a failure, and a recursion checks the stack: in a
    // hosted unit whose `main` may start a thread that state is thread-local; on the board, and
    // in a hosted program of one thread, an ordinary global. The unit reads the stack's limit in
    // place; since R1's 5b.8 it no longer reads the failure flag, which its callers learn of
    // from the failure bit.
    const ADD: &str =
        "fn add(a: Int, b: Int) -> Int { if a == 0 { b } else { add(a - 1, b + 1) } }\n";
    for (main, target, tls) in [
        (
            "fn main(n: NetCap) -> Int { add(1, 2) }\n",
            "x86_64-unknown-linux-gnu",
            true,
        ),
        (
            "fn main() -> Int { add(1, 2) }\n",
            "x86_64-unknown-linux-gnu",
            false,
        ),
        ("fn main() -> Int { add(1, 2) }\n", BOARD, false),
    ] {
        let d = program(&format!("{ADD}{main}"));
        let o = nazm(
            d.path(),
            &["build", "--target", target, "--objects", target, "prog.nz"],
        );
        assert!(o.status.success(), "{}", err(&o));
        let unit = d.path().join(target).join("00-m0.o");
        assert_eq!(tls_symbols(&unit) > 0, tls, "{target}");
        let runtime = d.path().join(target).join("01-runtime.o");
        assert_eq!(tls_symbols(&runtime) > 0, tls, "{target}");
    }
}

#[test]
fn what_a_board_cannot_have_is_refused_by_name() {
    let d = program(
        "fn main() -> Int { let v = ints_new(); ints_push(v, 4); \
         ints_len(v) + str_len(str_concat(\"a\", \"b\")) }\n",
    );
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    let e = err(&o);
    assert!(
        e.contains("`aarch64-unknown-none` has no operating system"),
        "{e}"
    );
    // Each part, with the symbols that reach it.
    assert!(e.contains("strings (`nz.str_alloc`"), "{e}");
    assert!(e.contains("), sequences (`nz.arr_grow`"), "{e}");
    assert!(
        !d.path().join("out").exists(),
        "objects were written for a refused build"
    );

    let d = program("fn main(io: IoCap) -> Int ! { io } { print(\"x\"); 0 }\n");
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    // `print` of a static string is a C library call: nothing allocates, but `write` is libc's.
    assert!(
        err(&o).contains("only a hosted runtime has: C library (`write`)"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_static_string_needs_no_runtime() {
    // A literal's length is a constant in the unit: nothing of the string runtime is reached.
    let d = program("fn main() -> Int { str_len(\"abc\") }\n");
    let o = nazm(
        d.path(),
        &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
    );
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn a_device_register_is_refused_where_there_is_none() {
    let d = program(PROGRAM);
    let o = nazm(d.path(), &["run", "prog.nz"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(e.contains("N0392"), "{e}");
    assert!(
        e.contains("`nazm run` cannot perform the built-in `mmio_write32`"),
        "{e}"
    );

    let o = nazm(d.path(), &["build", "prog.nz", "-o", "prog"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(e.contains("N0392"), "{e}");
    assert!(e.contains("a build for `"), "{e}");
    assert!(!d.path().join("prog").exists());

    let d = program("fn main(m: MmioCap) -> Int ! {} { mmio_read32(150994944) }\n");
    let o = nazm(d.path(), &["run", "prog.nz"]);
    assert!(
        err(&o).contains("the built-in `mmio_read32`"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_device_register_needs_an_mmio_cap_held() {
    let d = program("fn poke() -> Int ! {} { mmio_write32(0, 0) }\nfn main() -> Int { poke() }\n");
    let o = nazm(d.path(), &["check", "prog.nz"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(e.contains("N0369"), "{e}");
    assert!(e.contains("MmioCap"), "{e}");
    // Held where it is called, in every function: an undeclared helper is not bridged to its
    // caller's `MmioCap`, as it would be for `io`.
    let d = program(
        "fn poke() -> Int { mmio_write32(0, 0) }\nfn main(m: MmioCap) -> Int ! {} { poke() }\n",
    );
    let o = nazm(d.path(), &["check", "prog.nz"]);
    assert!(
        !o.status.success(),
        "an undeclared helper used its caller's MmioCap"
    );
    assert!(
        err(&o).contains("`poke` holds no `MmioCap` here"),
        "{}",
        err(&o)
    );
}

#[test]
fn cranelift_builds_no_board() {
    let d = program(PROGRAM);
    let o = nazm(
        d.path(),
        &[
            "build",
            "--backend",
            "cranelift",
            "--target",
            BOARD,
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert_eq!(o.status.code(), Some(5));
    assert!(
        err(&o).contains("`--backend cranelift` builds no freestanding target"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_board_image_needs_objects() {
    let d = program(PROGRAM);
    let o = nazm(d.path(), &["build", "--target", BOARD, "prog.nz"]);
    assert_eq!(o.status.code(), Some(4));
    assert!(err(&o).contains("--objects DIR"), "{}", err(&o));
}

/// Boots the image on QEMU's `virt` machine. Needs Docker and the `nazm-qemu:n62` image
/// (`docs/performance.md`, N62): `cargo test -p nazm-cli --test cli_e_l -- --ignored freestanding::`.
#[test]
#[ignore = "starts a container"]
fn the_image_boots_on_qemu_and_reports_on_the_uart() {
    let boot = |src: &str| {
        let d = program(src);
        let o = nazm(
            d.path(),
            &["build", "--target", BOARD, "--objects", "out", "prog.nz"],
        );
        assert!(o.status.success(), "{}", err(&o));
        let out = d.path().join("out");
        let o = Command::new("docker")
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{}:/w", out.display()))
            .args([
                "-w",
                "/w",
                "nazm-qemu:n62",
                "sh",
                "-c",
                "ld.lld -T link.ld 00-m0.o 01-runtime.o 02-entry.o -o /tmp/x.elf && \
                 timeout 20 qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none \
                 -semihosting -kernel /tmp/x.elf",
            ])
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    assert_eq!(boot(PROGRAM), (Some(0), "Hi\n100\n".to_owned()));
    let (code, out) = boot(
        "fn main() -> Int { let mut x = 9223372036854775800; \
         while x > 0 { x = x + 1; } x }\n",
    );
    assert_eq!(code, Some(2), "{out}");
    assert!(
        out.contains("error[N0400]: this addition overflows `Int`"),
        "{out}"
    );
    let (code, out) = boot(
        "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }\n\
         fn main() -> Int { down(100000000) }\n",
    );
    assert_eq!(code, Some(2), "{out}");
    assert!(out.contains("error[N0408]"), "{out}");
    // Q1-A1-01: a frame larger than the board's whole 64 KiB stack, in no call cycle, meets the
    // check before its prologue lays it out.
    let (code, out) = boot(
        "fn big(n: Int) -> Int { let mut a = [0; 4096]; let mut b = [0; 4096];\n\
         a[n] = 1; b[n] = 2; a[n] + b[n] }\n\
         fn main() -> Int { big(3) }\n",
    );
    assert_eq!(code, Some(2), "{out}");
    assert!(
        out.contains("error[N0408]") && out.contains("prog.nz:1:4"),
        "{out}"
    );
}
