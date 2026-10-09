//! Gate 2: the C foundation for other languages.
//!
//! `docs/general-purpose.md` §23. Python's `ctypes`, Swift, Kotlin/JNI, Node-API and C# P/Invoke all
//! bind to a C ABI of scalars; since Gate 2 an export takes and returns every number Nazm has, as
//! the C type the generated header names. What this holds: the header's declarations, and the same
//! answers at the edges of every width and both floats from a C program linking the static archive
//! and the shared library, and from Python's `ctypes` loading the shared library — under both
//! backends.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod differential;

use std::process::Command;

use differential::{codes, nazm};

const LIB: &str = "pub extern \"C\" fn nz_scale(x: Float64, k: Int32) -> Float64 ! {} = \"nz_scale\" { x * Float64(k) }\n\
pub extern \"C\" fn nz_mask(x: UInt8, y: UInt16) -> UInt32 ! {} = \"nz_mask\" { UInt32(x) | (UInt32(y) << 8) }\n\
pub extern \"C\" fn nz_half(x: Float32) -> Float32 ! {} = \"nz_half\" { x / Float32(2.0) }\n\
pub extern \"C\" fn nz_neg(n: Int8, m: Int16) -> Int32 ! {} = \"nz_neg\" { Int32(n) * Int32(m) }\n\
pub extern \"C\" fn nz_top(x: UInt64) -> UInt64 ! {} = \"nz_top\" { x ^ UInt64(1) }\n\
pub extern \"C\" fn nz_both(a: Int, b: Bool) -> Int ! {} = \"nz_both\" { if b { a } else { 0 - a } }\n";

const MAIN_C: &str = "#include <stdio.h>\n#include \"libnz.h\"\n\
int main(void) {\n\
  printf(\"%g %u %g %d %llu %lld\\n\", nz_scale(1.5, -3), nz_mask(255, 65535), (double)nz_half(3.0f),\n\
         nz_neg(-128, -32768), (unsigned long long)nz_top(18446744073709551615ULL), (long long)nz_both(7, false));\n\
  return 0;\n\
}\n";

const PYTHON: &str = "import ctypes, sys\n\
l = ctypes.CDLL(sys.argv[1])\n\
l.nz_scale.restype = ctypes.c_double; l.nz_scale.argtypes = [ctypes.c_double, ctypes.c_int32]\n\
l.nz_mask.restype = ctypes.c_uint32; l.nz_mask.argtypes = [ctypes.c_uint8, ctypes.c_uint16]\n\
l.nz_half.restype = ctypes.c_float; l.nz_half.argtypes = [ctypes.c_float]\n\
l.nz_neg.restype = ctypes.c_int32; l.nz_neg.argtypes = [ctypes.c_int8, ctypes.c_int16]\n\
l.nz_top.restype = ctypes.c_uint64; l.nz_top.argtypes = [ctypes.c_uint64]\n\
l.nz_both.restype = ctypes.c_int64; l.nz_both.argtypes = [ctypes.c_int64, ctypes.c_bool]\n\
print('%g %d %g %d %d %d' % (l.nz_scale(1.5, -3), l.nz_mask(255, 65535), l.nz_half(3.0), l.nz_neg(-128, -32768), l.nz_top(2**64 - 1), l.nz_both(7, False)))\n";

const ANSWERS: &str = "-4.5 16777215 1.5 4194304 18446744073709551614 -7";

fn shared_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libnz.dylib"
    } else {
        "libnz.so"
    }
}

#[test]
fn the_header_names_every_numbers_c_type() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("lib.nz"), LIB).unwrap();
    let b = nazm(d.path(), &["build", "--lib", "lib.nz", "-o", "libnz.a"]);
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let h = std::fs::read_to_string(d.path().join("libnz.h")).unwrap();
    for decl in [
        "double nz_scale(double a0, int32_t a1);",
        "uint32_t nz_mask(uint8_t a0, uint16_t a1);",
        "float nz_half(float a0);",
        "int32_t nz_neg(int8_t a0, int16_t a1);",
        "uint64_t nz_top(uint64_t a0);",
        "int64_t nz_both(int64_t a0, bool a1);",
    ] {
        assert!(h.contains(decl), "{decl}\n{h}");
    }
}

#[test]
fn c_and_python_get_the_same_answers_at_every_width_from_both_backends() {
    let python = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    for backend in ["llvm", "cranelift"] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("lib.nz"), LIB).unwrap();
        std::fs::write(d.path().join("main.c"), MAIN_C).unwrap();
        std::fs::write(d.path().join("bind.py"), PYTHON).unwrap();
        // The static archive, linked into a C program.
        let b = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "--lib",
                "lib.nz",
                "-o",
                "libnz.a",
            ],
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let mut link = Command::new("clang");
        link.current_dir(d.path())
            .args(["main.c", "libnz.a", "-o", "static"]);
        if cfg!(target_os = "linux") {
            link.args(["-lpthread", "-lm"]);
        }
        let o = link.output().unwrap();
        assert!(
            o.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let r = Command::new(d.path().join("static")).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&r.stdout).trim_end(),
            ANSWERS,
            "{backend} static"
        );
        // The shared library, from C and from Python.
        std::fs::remove_file(d.path().join("libnz.a")).unwrap();
        let s = nazm(
            d.path(),
            &[
                "build",
                "--backend",
                backend,
                "--lib",
                "--shared",
                "lib.nz",
                "-o",
                shared_name(),
            ],
        );
        assert_eq!(s.code, Some(0), "{backend}: {}", s.stderr);
        let o = Command::new("clang")
            .current_dir(d.path())
            .args(["main.c", "-L.", "-lnz", "-o", "dynamic"])
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let r = Command::new(d.path().join("dynamic"))
            .env("DYLD_LIBRARY_PATH", d.path())
            .env("LD_LIBRARY_PATH", d.path())
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&r.stdout).trim_end(),
            ANSWERS,
            "{backend} shared"
        );
        if common::skip_without(python, "python3 is not on PATH, so ctypes is not tried") {
            continue;
        }
        let r = Command::new("python3")
            .current_dir(d.path())
            .arg("bind.py")
            .arg(d.path().join(shared_name()))
            .output()
            .unwrap();
        assert!(
            r.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&r.stdout).trim_end(),
            ANSWERS,
            "{backend} python"
        );
    }
}

#[test]
fn an_export_still_takes_nothing_c_cannot_hold_by_value() {
    for ty in ["Str", "Bytes", "Vec[Int]", "Text"] {
        let src = format!("pub extern \"C\" fn f(x: {ty}) -> Int ! {{}} = \"f\" {{ 0 }}\nfn main() -> Int {{ 0 }}\n");
        assert!(
            codes(&src).contains(&"N0391".to_owned()),
            "{ty}: {:?}",
            codes(&src)
        );
    }
}
