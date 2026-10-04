//! N57: targets and cross-compilation.
//!
//! `docs/architecture.md` §7.59. What a whole build shows: every triple of the matrix is built by
//! both backends from this host — to an executable where the host's toolchain links it, to objects
//! and a link list where it cannot — each object in the target's own format and architecture; a
//! target the host cannot link is refused unless objects are asked for; objects for two targets are
//! never one cache entry; and a program built for another macOS architecture runs, under Rosetta,
//! where Rosetta is installed. Linux objects built on macOS are linked and run in the Linux
//! container by hand (`docs/performance.md`, N57), not here: a test does not start a container.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

use nazm_lir::backend::Target;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];

const PROGRAM: &str = "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
                       fn main(io: IoCap) -> Int ! { io } { print(int_to_str(fib(20))); 0 }\n";

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

/// What an object or executable says it is, from its header: format and architecture.
fn header(path: &Path) -> (&'static str, &'static str) {
    let b = std::fs::read(path).unwrap();
    if b.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) {
        let cpu = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        let arch = match cpu {
            0x0100_0007 => "x86_64",
            0x0100_000c => "aarch64",
            _ => "other",
        };
        ("mach-o", arch)
    } else if b.starts_with(b"\x7fELF") {
        let machine = u16::from_le_bytes([b[18], b[19]]);
        let arch = match machine {
            62 => "x86_64",
            183 => "aarch64",
            _ => "other",
        };
        ("elf", arch)
    } else {
        ("unknown", "unknown")
    }
}

fn expected(triple: &str) -> (&'static str, &'static str) {
    let format = if triple.contains("apple") {
        "mach-o"
    } else {
        "elf"
    };
    let arch = if triple.starts_with("x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };
    (format, arch)
}

#[test]
fn every_target_is_built_by_both_backends_in_its_own_format() {
    let host = Target::host().unwrap();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    for triple in TARGETS {
        let target = Target::parse(triple).unwrap();
        for backend in ["llvm", "cranelift"] {
            let dir = format!("obj-{triple}-{backend}");
            let o = nazm(
                d.path(),
                &[
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "--target",
                    triple,
                    "--objects",
                    &dir,
                    "prog.nz",
                ],
            );
            assert!(o.status.success(), "{triple} {backend}: {}", err(&o));
            let listed = std::fs::read_to_string(d.path().join(&dir).join("link.txt")).unwrap();
            assert!(listed.contains(&format!("target {triple}\n")), "{listed}");
            // The task pool's switch (N83) follows the runtime where the runtime has tasks or
            // channels, and the target's `errno` (N85) where C is called — both always under
            // Cranelift, whose runtime is its whole baseline.
            let want = if backend == "cranelift" {
                "objects 00-m0.o 01-runtime.o 02-switch.o 03-errno.o 04-entry.o\n"
            } else {
                "objects 00-m0.o 01-runtime.o 02-entry.o\n"
            };
            assert!(listed.contains(want), "{listed}");
            let libs = if triple.contains("linux") {
                "-lpthread"
            } else {
                ""
            };
            assert!(listed.ends_with(&format!("libraries {libs}\n")), "{listed}");
            // Every object listed, the switch's too, is for the target.
            let objects: Vec<&str> = listed
                .lines()
                .find_map(|l| l.strip_prefix("objects "))
                .unwrap()
                .split(' ')
                .collect();
            for o in objects {
                assert_eq!(
                    header(&d.path().join(&dir).join(o)),
                    expected(triple),
                    "{triple} {backend} {o}"
                );
            }
            // An executable only where the host's toolchain links the target.
            let exe = d.path().join(format!("exe-{triple}-{backend}"));
            let b = nazm(
                d.path(),
                &[
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "--target",
                    triple,
                    "prog.nz",
                    "-o",
                    exe.to_str().unwrap(),
                ],
            );
            if target.links_on(&host) {
                assert!(b.status.success(), "{triple} {backend}: {}", err(&b));
                assert_eq!(header(&exe), expected(triple), "{triple} {backend}");
            } else {
                assert_eq!(b.status.code(), Some(4), "{triple} {backend}");
                assert!(err(&b).contains("has no linker for"), "{}", err(&b));
                assert!(!exe.exists());
            }
        }
    }
}

#[test]
fn a_program_for_the_other_macos_architecture_runs_where_rosetta_does() {
    let host = Target::host().unwrap();
    if host.triple != "aarch64-apple-darwin" {
        return;
    }
    // Rosetta is an optional part of the host, not of the build; without it this is compile-only.
    let rosetta = Command::new("arch")
        .args(["-x86_64", "/usr/bin/true"])
        .status()
        .is_ok_and(|s| s.success());
    if !rosetta {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(format!("x86-{backend}"));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--target",
                "x86_64-apple-darwin",
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert!(b.status.success(), "{}", err(&b));
        let r = Command::new(&exe).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&r.stdout), "67650\n", "{backend}");
    }
}

#[test]
fn objects_for_two_targets_are_never_one_cache_entry() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let report = |o: &Output| -> serde_json::Value {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .find(|l| l.contains("nazm.build-report/1"))
            .map(|l| serde_json::from_str(l).unwrap())
            .unwrap_or_else(|| panic!("no build report: {}", err(o)))
    };
    for backend in ["llvm", "cranelift"] {
        let mut stored = Vec::new();
        for (i, triple) in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
            .iter()
            .enumerate()
        {
            let dir = format!("o-{backend}-{i}");
            let o = nazm(
                d.path(),
                &[
                    "build",
                    "--build-report",
                    "--backend",
                    backend,
                    "--target",
                    triple,
                    "--objects",
                    &dir,
                    "prog.nz",
                ],
            );
            assert!(o.status.success(), "{}", err(&o));
            stored.push(report(&o));
        }
        // The second target reuses nothing the first stored.
        assert_eq!(
            stored[1]["native"]["reused"].as_u64(),
            Some(0),
            "{backend}: {}",
            stored[1]
        );
        // And the first, built again, reuses its own.
        let again = nazm(
            d.path(),
            &[
                "build",
                "--build-report",
                "--backend",
                backend,
                "--target",
                "aarch64-unknown-linux-gnu",
                "--objects",
                "again",
                "prog.nz",
            ],
        );
        assert!(
            report(&again)["native"]["reused"].as_u64().unwrap() >= 1,
            "{backend}: {}",
            report(&again)
        );
    }
}

#[test]
fn a_triple_outside_the_matrix_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    for triple in [
        "riscv64gc-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "i686-unknown-linux-gnu",
    ] {
        let o = nazm(
            d.path(),
            &["build", "--target", triple, "--objects", "o", "prog.nz"],
        );
        assert_eq!(o.status.code(), Some(4), "{triple}");
        assert!(
            err(&o).contains("not a target this compiler generates code for"),
            "{}",
            err(&o)
        );
        assert!(!d.path().join("o").exists());
    }
}

#[test]
fn inspect_names_every_target_and_what_this_host_makes_of_it() {
    let host = Target::host().unwrap();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let o = nazm(d.path(), &["inspect", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let listed = v["toolchain"]["targets"].as_array().unwrap();
    // The four hosted triples and, since N62, the board, since N64, WebAssembly, and since N86 the
    // RISC-V board.
    assert_eq!(listed.len(), 7);
    for t in listed {
        let target = Target::parse(t["triple"].as_str().unwrap()).unwrap();
        let want = if target.links_on(&host) {
            "executable"
        } else {
            "objects"
        };
        assert_eq!(t["output"], want, "{t}");
    }
}
