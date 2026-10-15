//! N86: boards as data, a second architecture, device registers by width.
//!
//! `docs/architecture.md` §7.87. What a build shows on this host: every board's linker script,
//! runtime and entry are generated from its description, and differ exactly where the board does;
//! `riscv64gc-unknown-none-elf` is a target of the matrix, and a host whose `clang` has no RISC-V
//! code generator refuses it by name rather than reporting the compiler's fault; the four new
//! device-register widths are refused by `nazm run` and a hosted build (`N0392`) and need an
//! `MmioCap`. That both boards boot is the ignored test at the end, run by hand in the
//! `nazm-qemu:n86` container — a test does not start a container by itself.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use nazm_lir::runtime::board::{self, Isa, Stop, BOARDS};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const RISCV: &str = "riscv64gc-unknown-none-elf";

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
fn every_board_s_texts_are_generated_from_its_description_and_differ_where_it_does() {
    assert_eq!(BOARDS.len(), 3);
    for b in &BOARDS {
        assert!(!b.hardware, "{}: no board has run on hardware", b.triple);
        let script = board::linker_script(b);
        assert!(
            script.contains(&format!(". = {:#x};", b.load_address)),
            "{script}"
        );
        // `_start` is first: RISC-V's small-data sections and every unwind table are placed or
        // discarded by name, never left as orphans ahead of it.
        assert!(script.contains("*(.text.start) *(.text*)"), "{script}");
        // An M profile's vector table goes before even `_start` (Gate 3).
        assert_eq!(
            script.contains("KEEP(*(.vectors)) *(.text.start)"),
            b.isa == Isa::Thumbv7m,
            "{script}"
        );
        for small in ["*(.srodata*)", "*(.sdata*)", "*(.sbss*)"] {
            assert!(script.contains(small), "{script}");
        }
        assert!(script.contains("/DISCARD/ : { *(.eh_frame*) }"), "{script}");
        let rt = board::emit_runtime(b, true);
        assert!(
            rt.contains(&format!(
                "store volatile i8 %c, ptr inttoptr (i64 {} to ptr)",
                b.uart
            )),
            "{rt}"
        );
        assert!(!rt.contains("thread_local"), "{rt}");
        // Every width of device register, each one volatile access of exactly that width.
        for (bits, ty) in [(8, "i8"), (16, "i16"), (32, "i32")] {
            assert!(rt.contains(&format!("@nz.mmio_read{bits}(")), "{rt}");
            assert!(rt.contains(&format!("load volatile {ty}, ptr %p")), "{rt}");
            assert!(
                rt.contains(&format!("store volatile {ty} %t, ptr %p")),
                "{rt}"
            );
        }
        let entry = board::emit_entry(b, "@m", 0, true);
        match (b.isa, b.stop) {
            (Isa::Aarch64, Stop::Semihosting) => {
                assert!(rt.contains("hlt #0xf000"), "{rt}");
                assert!(entry.contains("ldr x0, =__stack_top"), "{entry}");
                assert!(entry.contains("msr cpacr_el1"), "{entry}");
                assert!(entry.contains("bl nz_board_main"), "{entry}");
            }
            (Isa::Riscv64, Stop::SifiveTest { address }) => {
                // 0x5555 for success, `code << 16 | 0x3333` for a failure.
                assert!(rt.contains("select i1 %ok, i32 21845, i32 %fail"), "{rt}");
                assert!(rt.contains("or i32 %shifted, 13107"), "{rt}");
                assert!(
                    rt.contains(&format!(
                        "store volatile i32 %word, ptr inttoptr (i64 {address} to ptr)"
                    )),
                    "{rt}"
                );
                assert!(!rt.contains("hlt"), "{rt}");
                assert!(entry.contains("la sp, __stack_top"), "{entry}");
                assert!(entry.contains("csrs mstatus, t0"), "{entry}");
                assert!(entry.contains("sd t2, 0(t0)"), "{entry}");
                assert!(entry.contains("call nz_board_main"), "{entry}");
            }
            (Isa::Thumbv7m, Stop::Semihosting) => {
                // AArch32's semihosting on an M profile, a block of words; the 64-bit operations
                // the core has no instruction for; the CMSDK UART enabled before `main`.
                assert!(rt.contains("bkpt 0xab"), "{rt}");
                assert!(!rt.contains("hlt"), "{rt}");
                for helper in ["@__atomic_compare_exchange_8(", "@__aeabi_ldivmod("] {
                    assert!(rt.contains(helper), "{helper}: {rt}");
                }
                assert!(rt.contains("define hidden void @nz.uart_init()"), "{rt}");
                assert!(entry.contains(".word __stack_top"), "{entry}");
                assert!(entry.contains("call void @nz.uart_init()"), "{entry}");
                assert!(entry.contains("bl nz_board_main"), "{entry}");
            }
            other => panic!("a board this test does not know: {other:?}"),
        }
    }
    assert_eq!(
        board::board(RISCV).map(|b| b.load_address),
        Some(0x8000_0000)
    );
    assert!(board::board("x86_64-unknown-none").is_none());
}

#[test]
fn inspect_lists_the_riscv_board_and_every_board_s_description() {
    let d = program("fn main() -> Int { 0 }\n");
    let o = nazm(d.path(), &["inspect", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let targets = v["toolchain"]["targets"].as_array().unwrap();
    let riscv = targets
        .iter()
        .find(|t| t["triple"] == RISCV)
        .unwrap_or_else(|| panic!("{targets:?}"));
    assert_eq!(riscv["output"], "objects");
    let boards = v["toolchain"]["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 3);
    assert_eq!(boards[1]["triple"], RISCV);
    assert_eq!(boards[1]["load_address"], "0x80000000");
    assert_eq!(boards[1]["uart"], "0x10000000");
    assert_eq!(boards[1]["stop"]["kind"], "sifive-test");
    assert_eq!(boards[0]["stop"]["kind"], "arm-semihosting");
    assert!(boards.iter().all(|b| b["hardware"] == false));
}

/// Whether this host's `clang` lists a RISC-V code generator.
fn host_generates_riscv() -> bool {
    Command::new("clang")
        .arg("-print-targets")
        .output()
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.split_whitespace().next() == Some("riscv64"))
        })
}

#[test]
fn a_riscv_board_is_built_where_clang_can_and_refused_by_name_where_it_cannot() {
    let d = program(
        "fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write8(268435456, c) }\n\
         fn main(m: MmioCap) -> Int ! {} { put(m, 72); 0 }\n",
    );
    let o = nazm(
        d.path(),
        &["build", "--target", RISCV, "--objects", "out", "prog.nz"],
    );
    if host_generates_riscv() {
        assert!(o.status.success(), "{}", err(&o));
        let listed = std::fs::read_to_string(d.path().join("out/link.txt")).unwrap();
        assert!(listed.contains(&format!("target {RISCV}\n")), "{listed}");
        // ELF, `EM_RISCV` (243).
        let obj = std::fs::read(d.path().join("out/00-m0.o")).unwrap();
        assert!(obj.starts_with(b"\x7fELF"));
        assert_eq!(u16::from_le_bytes([obj[18], obj[19]]), 243);
    } else {
        assert_eq!(o.status.code(), Some(4));
        assert!(
            err(&o).contains("has no RISC-V code generator"),
            "{}",
            err(&o)
        );
        assert!(!d.path().join("out").exists());
    }
    // Cranelift builds no board, RISC-V's included.
    let c = nazm(
        d.path(),
        &[
            "build",
            "--backend",
            "cranelift",
            "--target",
            RISCV,
            "--objects",
            "out",
            "prog.nz",
        ],
    );
    assert_eq!(c.status.code(), Some(5));
    assert!(
        err(&c).contains("`--backend cranelift` builds no freestanding target"),
        "{}",
        err(&c)
    );
}

/// A driver on `PATH` whose `-print-targets` lists RISC-V only when `riscv` is true, and which
/// refuses every other request — so the question N86 asks has the same answer on every host.
#[cfg(unix)]
fn fake_clang(riscv: bool) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let extra = if riscv {
        "    riscv64    - 64-bit RISC-V\n"
    } else {
        ""
    };
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = -print-targets ]; then\n\
         printf '  Registered Targets:\\n    aarch64    - AArch64 (little endian)\\n{extra}    x86-64     - 64-bit X86: EM64T and AMD64\\n'\n\
         exit 0\n\
         fi\n\
         echo 'fake clang: refused' >&2\n\
         exit 1\n"
    );
    let path = d.path().join("clang");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    d
}

/// N86, decided by the driver's answer rather than by which `clang` the host happens to have: a
/// driver without a RISC-V code generator is refused by name before anything is compiled; one that
/// has it is not refused for it (here it then fails to compile, being a fake — a different failure).
#[test]
#[cfg(unix)] // the driver is an `sh` script
fn a_riscv_board_is_refused_by_name_exactly_when_the_driver_has_no_riscv_generator() {
    let d = program(
        "fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write8(268435456, c) }\n\
         fn main(m: MmioCap) -> Int ! {} { put(m, 72); 0 }\n",
    );
    for riscv in [false, true] {
        let fake = fake_clang(riscv);
        let path = format!(
            "{}:{}",
            fake.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = format!("out-{riscv}");
        let o = Command::new(NAZM)
            .current_dir(d.path())
            .env("PATH", path)
            .args(["build", "--target", RISCV, "--objects", &out, "prog.nz"])
            .output()
            .unwrap();
        let named = err(&o).contains("has no RISC-V code generator");
        assert_eq!(o.status.code(), Some(4), "riscv={riscv}: {}", err(&o));
        assert_eq!(named, !riscv, "riscv={riscv}: {}", err(&o));
        if !riscv {
            assert!(!d.path().join(&out).exists(), "nothing is written");
        }
    }
}

#[test]
fn a_device_register_of_any_width_is_a_board_s_and_needs_an_mmio_cap() {
    for (call, width) in [
        ("mmio_read8(4096)", "mmio_read8"),
        ("mmio_write8(4096, 1)", "mmio_write8"),
        ("mmio_read16(4096)", "mmio_read16"),
        ("mmio_write16(4096, 1)", "mmio_write16"),
    ] {
        let d = program(&format!("fn main(m: MmioCap) -> Int ! {{}} {{ {call} }}\n"));
        let run = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
        assert!(!run.status.success());
        assert!(
            err(&run).contains("N0392") && err(&run).contains(width),
            "{}",
            err(&run)
        );
        let hosted = nazm(d.path(), &["build", "prog.nz", "-o", "x"]);
        assert!(!hosted.status.success());
        assert!(err(&hosted).contains("N0392"), "{}", err(&hosted));
        // A board builds it.
        let board = nazm(
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
        assert!(board.status.success(), "{}", err(&board));
        // And without an `MmioCap` it is refused wherever it is built.
        let d = program(&format!(
            "fn f() -> Int ! {{}} {{ {call} }}\nfn main() -> Int {{ 0 }}\n"
        ));
        let checked = nazm(d.path(), &["check", "prog.nz"]);
        assert!(!checked.status.success());
        assert!(err(&checked).contains("MmioCap"), "{}", err(&checked));
    }
}

/// Builds `nazm` from a copy of this tree inside the `nazm-qemu:n86` image, then builds, links
/// and boots programs on both boards at `-O0` and `-O2` (`docs/performance.md`, N86). Needs Docker
/// and the image: `docker build -t nazm-qemu:n86 -f docker/qemu.Dockerfile docker`, then
/// `cargo test -p nazm-cli --test cli_a_b -- --ignored boards::`.
#[test]
#[ignore = "starts a container"]
fn both_boards_boot_and_agree_under_qemu() {
    let ev = tempfile::tempdir().unwrap();
    let w = |name: &str, text: &str| std::fs::write(ev.path().join(name), text).unwrap();
    w(
        "hello_a64.nz",
        "fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write8(150994944, c) }\n\
         fn main(m: MmioCap) -> Int ! {} { put(m, 72); put(m, 329); put(m, 10); 100 }\n",
    );
    w(
        "hello_rv.nz",
        "fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write8(268435456, c) }\n\
         fn main(m: MmioCap) -> Int ! {} { put(m, 72); put(m, 329); put(m, 10); 100 }\n",
    );
    w(
        "overflow.nz",
        "fn main() -> Int { let mut x = 9223372036854775800; while x > 0 { x = x + 1; } x }\n",
    );
    w(
        "deep.nz",
        "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }\n\
         fn main() -> Int { down(100000000) }\n",
    );
    let script = r#"set -eu
mkdir -p /work/n86
cd /src && tar --exclude=./target --exclude=./.git -cf - . | (cd /work/n86 && tar xf -)
cd /work/n86 && CARGO_TARGET_DIR=/work/target cargo build -q -p nazm-cli --offline
N=/work/target/debug/nazm
cd /ev
for t in aarch64-unknown-none riscv64gc-unknown-none-elf; do
  case $t in
    aarch64*) h=hello_a64; q="qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none -semihosting -kernel";;
    *) h=hello_rv; q="qemu-system-riscv64 -M virt -bios none -nographic -nic none -kernel";;
  esac
  for o in 0 2; do
    for p in $h overflow deep; do
      d=/tmp/o-$t-$p-$o
      $N build --opt-level $o --target $t --objects $d $p.nz > /dev/null
      (cd $d && ld.lld -T link.ld $(sed -n 's/^objects //p' link.txt) -o x.elf)
      set +e; out=$(timeout 20 $q $d/x.elf 2>&1); code=$?; set -e
      printf '%s %s %s %s %s\n' "$t" "$p" "$o" "$code" "$(printf '%s' "$out" | tr '\n' '|')"
    done
  done
done
"#;
    w("run.sh", script);
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
    assert!(o.status.success(), "{}", err(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    for t in ["aarch64-unknown-none", RISCV] {
        for opt in ["0", "2"] {
            let line = |p: &str| {
                out.lines()
                    .find(|l| l.starts_with(&format!("{t} {p} {opt} ")))
                    .unwrap_or_else(|| panic!("{t} {p} -O{opt}: {out}"))
                    .to_owned()
            };
            let h = if t == RISCV { "hello_rv" } else { "hello_a64" };
            // The byte write truncates 0x149 to `I`.
            assert!(line(h).ends_with(" 0 HI|100"), "{}", line(h));
            assert!(
                line("overflow").contains(" 2 nazm: error[N0400]"),
                "{}",
                line("overflow")
            );
            assert!(
                line("deep").contains(" 2 nazm: error[N0408]"),
                "{}",
                line("deep")
            );
        }
    }
}
