//! `nazm build --release` (R1.5, D15; `docs/architecture.md` §7.117): a macOS executable stripped,
//! position-independent, stack-protected and signed ad hoc with the hardened runtime — a test
//! per property, read from the file with the system's own tools — and the default build unchanged.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// A function with a local array, which a stack canary guards.
const PROGRAM: &str = "fn sum(n: Int): Int {
    let a: [Int; 8] = [1, 2, 3, 4, 5, 6, 7, 8]
    let mut t = 0
    let mut i = 0
    while i < 8 {
        t += a[i] * n
        i += 1
    }
    t
}

fn main() {
    println(sum(2))
}
";

fn nazm(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(NAZM)
        .args(args)
        .env("NAZM_USER_CACHE_DIR", "")
        .current_dir(dir)
        .output()
        .expect("nazm")
}

/// The program built into `dir` as `name`, with `extra` arguments; it runs and prints `72`.
fn built(dir: &Path, name: &str, extra: &[&str]) -> PathBuf {
    std::fs::write(dir.join("main.nz"), PROGRAM).unwrap();
    let mut args = vec!["build", "main.nz", "-o", name];
    args.extend_from_slice(extra);
    let out = nazm(dir, &args);
    assert!(
        out.status.success(),
        "nazm {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exe = dir.join(name);
    let ran = Command::new(&exe).output().expect("run");
    assert!(String::from_utf8_lossy(&ran.stdout).starts_with("72\n"));
    assert!(ran.status.success());
    exe
}

/// What a tool prints about `exe`, standard output and error together (`codesign` writes its
/// description to standard error).
fn tool(name: &str, args: &[&str], exe: &Path) -> String {
    let out = Command::new(name).args(args).arg(exe).output().expect(name);
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The release build and the default one of [`PROGRAM`], side by side.
fn both() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let release = built(d.path(), "hardened", &["--release"]);
    let default = built(d.path(), "plain", &[]);
    (d, release, default)
}

#[cfg(target_os = "macos")]
#[test]
fn a_release_executable_has_no_symbols_or_debug_information_left() {
    let (_d, release, default) = both();
    // `nm -U`: the symbols the file defines. The default build names its functions.
    let plain = tool("nm", &["-U"], &default);
    assert!(plain.contains("_main"), "{plain}");
    let hardened = tool("nm", &["-U"], &release);
    assert!(
        !hardened.contains("_main") && !hardened.contains("nz"),
        "a release build names none of its functions: {hardened}"
    );
    // `nm -a` lists debug-map entries too: none is left.
    let all = tool("nm", &["-a"], &release);
    assert!(
        !all.contains(" OSO ") && !all.contains(" SO "),
        "no debug map: {all}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_release_executable_is_position_independent() {
    let (_d, release, _) = both();
    let header = tool("otool", &["-hv"], &release);
    assert!(header.contains(" PIE"), "{header}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_release_executable_checks_a_stack_canary_and_the_default_one_does_not() {
    let (_d, release, default) = both();
    // `__stack_chk_fail` is what a protected function calls when its canary was overwritten.
    let hardened = tool("nm", &["-u"], &release);
    assert!(hardened.contains("___stack_chk_fail"), "{hardened}");
    assert!(hardened.contains("___stack_chk_guard"), "{hardened}");
    let plain = tool("nm", &["-u"], &default);
    assert!(!plain.contains("___stack_chk"), "{plain}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_release_executable_is_signed_ad_hoc_with_the_hardened_runtime_and_the_default_one_is_not() {
    let (_d, release, default) = both();
    let signed = tool("codesign", &["-dv"], &release);
    assert!(
        signed.contains("adhoc") && signed.contains("runtime"),
        "{signed}"
    );
    let verify = Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(&release)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let plain = tool("codesign", &["-dv"], &default);
    assert!(!plain.contains("runtime"), "{plain}");
}

#[cfg(target_os = "macos")]
#[test]
fn two_release_builds_are_the_same_bytes() {
    // N46: the strip and the signature add nothing a second build would not.
    let d = tempfile::tempdir().unwrap();
    let a = built(d.path(), "hardened", &["--release", "--no-cache"]);
    let first = std::fs::read(&a).unwrap();
    let b = built(d.path(), "hardened", &["--release", "--no-cache"]);
    assert_eq!(first, std::fs::read(b).unwrap());
}

#[test]
fn release_is_refused_where_it_cannot_hold() {
    // Cranelift has no stack protector; a debug build keeps what a release one strips; a library
    // and objects are not executables; another target is not this host's macOS.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.nz"), PROGRAM).unwrap();
    for (extra, why) in [
        (&["--backend", "cranelift"][..], "--backend llvm"),
        // The other macOS architecture links here, so only `--release` refuses it.
        (&["--target", "x86_64-apple-darwin"][..], "another target"),
        (&["--objects", "o"][..], "--objects"),
        (&["--debug"][..], "--debug"),
        (&["--lib"][..], "--lib"),
    ] {
        let mut args = vec!["build", "main.nz", "--release"];
        args.extend_from_slice(extra);
        let out = nazm(d.path(), &args);
        assert!(!out.status.success(), "{args:?} was built");
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(said.contains(why), "{args:?}: {said}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_provenance_record_names_a_release_build_and_only_a_release_build() {
    let d = tempfile::tempdir().unwrap();
    built(
        d.path(),
        "hardened",
        &["--release", "--provenance", "r.json"],
    );
    built(d.path(), "plain", &["--provenance", "p.json"]);
    let read = |f: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(d.path().join(f)).unwrap()).unwrap()
    };
    assert_eq!(
        read("r.json")["backend"]["release"],
        serde_json::json!(true)
    );
    assert!(read("p.json")["backend"].get("release").is_none());
}
