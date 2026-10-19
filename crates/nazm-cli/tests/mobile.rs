//! Gate 3: the mobile targets and the desktop window host (`docs/user/spec.md`, *Mobile*;
//! `docs/internal/systems-domains.md` Parts P and Q).
//!
//! Library targets over the stable C ABI. **Android ARM64** (`aarch64-linux-android`): objects for
//! the NDK's `clang`, a static Bionic executable of a C host and a Nazm library — and of a whole Nazm
//! program, its task pool included — run on an aarch64 **Linux kernel** in a container: Android's C
//! library on Linux, never an Android device or emulator, and labelled so. **iOS ARM64**
//! (`aarch64-apple-ios`): objects, a static archive and a C host linked against Xcode's iOS SDK —
//! **link-verified only**: no simulator runtime is installed, and nothing here claims an iOS run.
//! **Desktop**: a Cocoa window whose event loop calls a Nazm shared library through the C ABI,
//! driven by synthetic clicks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

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

const LIB: &str = "fn square(x: Int) -> Int ! {} { x * x }\n\
pub extern \"C\" fn sum_squares(a: Int, b: Int) -> Int ! {} = \"nazm_sum_squares\" { square(a) + square(b) }\n\
pub extern \"C\" fn label_len(n: Int) -> Int ! {} = \"nazm_label_len\" { str_len(str_concat(\"item-\", int_to_str(n))) }\n";

const HOST: &str = "#include <stdio.h>\n#include <stdint.h>\n\
int64_t nazm_sum_squares(int64_t, int64_t);\nint64_t nazm_label_len(int64_t);\n\
int main(void) { printf(\"%lld %lld\\n\", (long long)nazm_sum_squares(3, 4), (long long)nazm_label_len(12345)); return 0; }\n";

const APP: &str = "use \"@std/io\";\nuse \"@std/fs\";\nuse \"@std/time\";\n\
fn work(n: Int, out: Chan[Int]) -> Int ! {} {\n\
    let mut s = 0;\n    let mut i = 0;\n    while i < n { s = s + i; i = i + 1; }\n\
    chan_send_of(out, s);\n    0\n}\n\
fn main(io: IoCap, t: SpawnCap, time: TimeCap) -> Int ! { io, spawn } {\n\
    let out = chan_new_of[Int](8);\n\
    scope {\n        let mut k = 1;\n        while k <= 4 { spawn work(k * 1000, out); k = k + 1; }\n    }\n\
    let got = vec_new[Int]();\n    let mut total = 0;\n    let mut j = 0;\n\
    while j < 4 { if chan_recv_of(out, got) { total = total + vec_pop(got); } j = j + 1; }\n\
    io_println(io, str_concat(\"total \", int_to_str(total)));\n\
    match fs_write(io, \"/tmp/nz.txt\", \"android\\n\") { Result.Ok(value: n) => {}, Result.Err(error: e) => { io_println(io, e); }, };\n\
    match fs_read(io, \"/tmp/nz.txt\") { Result.Ok(value: s) => { io_println(io, str_concat(\"read \", str_slice(s, 0, 7))); }, Result.Err(error: e) => { io_println(io, e); }, };\n\
    io_println(io, if time_ms(time) > 0 { \"clock ok\" } else { \"clock wrong\" });\n\
    args_count()\n}\n";

/// `src`'s objects for `target` in `dir/out`, a library with `lib`.
fn objects(dir: &Path, src: &str, target: &str, lib: bool) -> Vec<String> {
    std::fs::write(dir.join("prog.nz"), src).unwrap();
    let mut args = vec!["build", "--target", target, "--objects", "out", "prog.nz"];
    if lib {
        args.insert(1, "--lib");
    }
    let o = nazm(dir, &args);
    assert!(o.status.success(), "{target}: {}", err(&o));
    let link = std::fs::read_to_string(dir.join("out/link.txt")).unwrap();
    assert!(link.contains(&format!("target {target}")), "{link}");
    link.lines()
        .find_map(|l| l.strip_prefix("objects "))
        .unwrap()
        .split(' ')
        .map(|o| format!("out/{o}"))
        .collect()
}

#[test]
fn both_mobile_targets_build_objects_for_their_toolchains() {
    for target in ["aarch64-linux-android", "aarch64-apple-ios"] {
        let d = tempfile::tempdir().unwrap();
        let objs = objects(d.path(), LIB, target, true);
        assert_eq!(
            objs.len(),
            2,
            "{target}: the library's module and the runtime"
        );
        let d = tempfile::tempdir().unwrap();
        let objs = objects(d.path(), APP, target, false);
        // A program: its modules, the runtime, the task switch, the stack, errno, the platform
        // unit and the entry.
        for unit in [
            "runtime.o",
            "switch.o",
            "stack.o",
            "errno.o",
            "os.o",
            "entry.o",
        ] {
            assert!(
                objs.iter().any(|o| o.ends_with(unit)),
                "{target}: {unit} in {objs:?}"
            );
        }
        // Each C library's own names: Bionic's `errno` and XSI `strerror_r`, Darwin's `__error`.
        let has = |unit: &str, name: &str| {
            let path = objs.iter().find(|o| o.ends_with(unit)).unwrap();
            let bytes = std::fs::read(d.path().join(path)).unwrap();
            let name = format!("{name}\0");
            bytes.windows(name.len()).any(|w| w == name.as_bytes())
        };
        if target.contains("android") {
            assert!(has("errno.o", "__errno") && !has("errno.o", "__errno_location"));
            assert!(has("os.o", "strerror_r") && !has("os.o", "__xpg_strerror_r"));
        } else {
            assert!(has("errno.o", "___error"));
        }
        // The LLVM backend's alone.
        let o = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                "cranelift",
                "--target",
                target,
                "--objects",
                "c",
                "prog.nz",
            ],
        );
        assert!(!o.status.success(), "{target}: {}", err(&o));
        assert!(err(&o).contains("builds no mobile target"), "{}", err(&o));
        let link = std::fs::read_to_string(d.path().join("out/link.txt")).unwrap();
        let libs = if target.contains("android") {
            "-lm"
        } else {
            ""
        };
        assert!(link.contains(&format!("libraries {libs}\n")), "{link}");
    }
}

/// The NDK, downloaded into `target/toolchains` (never installed system-wide), if it is there.
fn ndk() -> Option<PathBuf> {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/toolchains/android-ndk-r27c/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android30-clang");
    bin.exists().then_some(bin)
}

/// Needs the NDK in `target/toolchains` and Docker: Bionic on an aarch64 Linux kernel.
#[test]
#[ignore = "links with the NDK and starts a container"]
fn a_static_bionic_executable_runs_a_nazm_library_and_a_whole_program_on_linux() {
    let Some(clang) = ndk() else {
        panic!("the NDK is not in target/toolchains");
    };
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.c"), HOST).unwrap();
    let objs = objects(d.path(), LIB, "aarch64-linux-android", true);
    let o = Command::new(&clang)
        .current_dir(d.path())
        .args(["-static", "main.c"])
        .args(&objs)
        .args(["-lm", "-o", "host"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let e = tempfile::tempdir().unwrap();
    let objs = objects(e.path(), APP, "aarch64-linux-android", false);
    let o = Command::new(&clang)
        .current_dir(e.path())
        .arg("-static")
        .args(&objs)
        .args(["-lm", "-o", "app"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    std::fs::copy(e.path().join("app"), d.path().join("app")).unwrap();
    let run = Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/w", d.path().display()))
        .args(["debian:bookworm-slim", "sh", "-c", "/w/host && /w/app x y"])
        .output()
        .unwrap();
    assert!(run.status.success(), "{}", err(&run));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "25 10\ntotal 14995000\nread android\nclock ok\n2\n"
    );
}

/// Needs Xcode (`/Applications/Xcode.app`): the iOS SDK. Linked, never run — no simulator runtime
/// is installed here.
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "links against Xcode's iOS SDK")]
fn an_ios_archive_and_program_link_against_the_ios_sdk() {
    let xcode = Path::new("/Applications/Xcode.app/Contents/Developer");
    if common::skip_without(xcode.exists(), "no Xcode at /Applications/Xcode.app") {
        return;
    }
    let xcrun = |dir: &Path, args: &[&str]| {
        Command::new("xcrun")
            .env("DEVELOPER_DIR", xcode)
            .current_dir(dir)
            .args(["--sdk", "iphoneos"])
            .args(args)
            .output()
            .unwrap()
    };
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.c"), HOST).unwrap();
    let objs = objects(d.path(), LIB, "aarch64-apple-ios", true);
    let mut ar = vec!["libtool", "-static", "-o", "libnazm.a"];
    ar.extend(objs.iter().map(String::as_str));
    let o = xcrun(d.path(), &ar);
    assert!(o.status.success(), "{}", err(&o));
    let o = xcrun(
        d.path(),
        &[
            "clang",
            "-target",
            "arm64-apple-ios17.0",
            "main.c",
            "libnazm.a",
            "-o",
            "host",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let e = tempfile::tempdir().unwrap();
    let objs = objects(e.path(), APP, "aarch64-apple-ios", false);
    let mut link = vec!["clang", "-target", "arm64-apple-ios17.0"];
    link.extend(objs.iter().map(String::as_str));
    link.extend(["-o", "app"]);
    let o = xcrun(e.path(), &link);
    assert!(o.status.success(), "{}", err(&o));
    // Each is an iOS executable: platform 2 in its build version.
    for exe in [d.path().join("host"), e.path().join("app")] {
        let o = Command::new("otool").arg("-l").arg(&exe).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("platform 2"), "{}: {text}", exe.display());
    }
}

/// Needs a macOS session that can open a window.
#[test]
#[ignore = "opens a window"]
fn a_cocoa_window_runs_on_a_nazm_library_through_the_c_abi() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/desktop/counter");
    let d = tempfile::tempdir().unwrap();
    for f in ["counter.nz", "host.m"] {
        std::fs::copy(src.join(f), d.path().join(f)).unwrap();
    }
    let o = nazm(
        d.path(),
        &[
            "build",
            "--lib",
            "--shared",
            "counter.nz",
            "-o",
            "libcounter.dylib",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let o = Command::new("clang")
        .current_dir(d.path())
        .args([
            "-fobjc-arc",
            "host.m",
            "-framework",
            "Cocoa",
            "-L.",
            "-lcounter",
            "-Wl,-rpath,@executable_path",
            "-o",
            "counter_host",
        ])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let run = common::run_bounded(
        Command::new(d.path().join("counter_host")).arg("--auto"),
        "counter_host",
    );
    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(
        run.stdout,
        "0 up\n1 up\n2 up\n3 up\n3 down\n2 down\n3 down\n4 down\n0 up\n"
    );
}
