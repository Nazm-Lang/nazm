//! N55: FFI v2 — C strings in, exported functions out.
//!
//! `docs/architecture.md` §7.57 and the spec section *Foreign functions v2*. What only a whole
//! build shows: that a `Str` crosses into C as a borrowed NUL-terminated copy under both
//! backends, with identical results and a balanced memory report, and that one holding a NUL is
//! refused before C runs; that an export is checked by its own rules; and that `nazm build
//! --lib` writes an archive and a header a C program links against, under both backends, the
//! same bytes on every build, with a Nazm failure inside an export ending the process instead of
//! unwinding into C.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The C side of the string tests: it reads what it is given and keeps nothing.
const C: &str = r#"
#include <stdint.h>
#include <string.h>
int64_t c_strlen(const char *s) { return (int64_t)strlen(s); }
int64_t c_byte_at(const char *s, int64_t i) { return (int64_t)(unsigned char)s[i]; }
int64_t c_cmp(const char *a, const char *b) { int r = strcmp(a, b); return r < 0 ? -1 : r > 0 ? 1 : 0; }
"#;

struct Dir {
    dir: tempfile::TempDir,
}

impl Dir {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn clang(&self, args: &[&str]) {
        let r = Command::new("clang")
            .current_dir(self.dir.path())
            .args(args)
            .output()
            .expect("clang");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    }

    fn nazm(&self, args: &[&str]) -> Output {
        Command::new(NAZM)
            .current_dir(self.dir.path())
            .args(args)
            .output()
            .expect("nazm")
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn codes(o: &Output) -> Vec<String> {
    stderr(o)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect()
}

fn check_codes(src: &str) -> Vec<String> {
    let d = Dir::new(&[("prog.nz", src)]);
    codes(&d.nazm(&["check", "--json", "prog.nz"]))
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(exe: &Path, args: &[&str]) -> Ran {
    let o = common::bounded_exe(exe)
        .args(args)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .output()
        .expect("run");
    Ran {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// `prog.nz` built against the C fixture by both backends and run: they must agree exactly.
fn both(src: &str) -> Ran {
    let d = Dir::new(&[("prog.nz", src), ("fixture.c", C)]);
    d.clang(&["-c", "-O1", "fixture.c", "-o", "fixture.o"]);
    let mut out = Vec::new();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path(&format!("prog-{backend}"));
        let b = d.nazm(&[
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--link",
            "fixture.o",
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ]);
        assert!(b.status.success(), "{backend}: {}", stderr(&b));
        out.push(run(&exe, &[]));
    }
    assert_eq!(out[0], out[1], "LLVM and Cranelift disagree");
    out.remove(0)
}

const DECLS: &str = "extern \"C\" fn c_strlen(s: Str) -> Int = \"c_strlen\";\n\
                     extern \"C\" fn c_byte_at(s: Str, i: Int) -> Int = \"c_byte_at\";\n\
                     extern \"C\" fn c_cmp(a: Str, b: Str) -> Int = \"c_cmp\";\n";

// --- strings into C ---------------------------------------------------------------------------

#[test]
fn a_str_crosses_into_c_as_a_borrowed_c_string_under_both_backends() {
    let r = both(&format!(
        "{DECLS}fn main(io: IoCap, f: ForeignCap) -> Int ! {{ io, foreign }} {{\n    \
         let s = str_concat(\"hel\", \"lo\");\n    \
         let mut i = 0;\n    let mut total = 0;\n    \
         while i < 1000 {{ total = total + c_strlen(s); i = i + 1; }}\n    \
         print(int_to_str(total)); print(\" \");\n    \
         print(int_to_str(c_byte_at(str_slice(s, 1, 3), 1))); print(\" \");\n    \
         print(int_to_str(c_strlen(\"\"))); print(\" \");\n    \
         print(int_to_str(c_cmp(\"abc\", str_concat(\"ab\", \"d\"))));\n    0\n}}\n"
    ));
    assert_eq!(r.code, Some(0), "{r:?}");
    // A slice is copied by its own length: `"el"`'s second byte is `l`, not the rest of `s`.
    assert_eq!(r.stdout, "5000 108 0 -10\n");
    let line = r
        .stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap();
    assert!(
        line.contains("strings allocated=6 reclaimed=6 live=0"),
        "{line}"
    );
}

#[test]
fn a_str_holding_a_nul_is_refused_before_c_runs() {
    let r = both(&format!(
        "{DECLS}fn main(io: IoCap, f: ForeignCap) -> Int ! {{ io, foreign }} {{\n    \
         print(\"before \");\n    \
         let n = c_cmp(\"ok\", str_concat(\"a\", str_from_byte(0)));\n    \
         print(\"after\");\n    n\n}}\n"
    ));
    assert_eq!(r.code, Some(2), "{r:?}");
    assert_eq!(r.stdout, "before ");
    assert!(
        r.stderr.contains("N0405") && r.stderr.contains("NUL byte"),
        "{r:?}"
    );
}

#[test]
fn a_str_result_from_c_is_a_copy_since_n85_and_a_sequence_result_is_still_refused() {
    // N85 (§7.86): C keeps the string it returns and Nazm copies it, so who frees it is
    // settled. A sequence is still nothing C can return.
    assert!(
        check_codes("extern \"C\" fn name() -> Str = \"c_name\";\nfn main() -> Int { 0 }\n")
            .is_empty()
    );
    assert_eq!(
        check_codes("extern \"C\" fn name() -> Ints = \"c_name\";\nfn main() -> Int { 0 }\n"),
        ["N0381"]
    );
}

// --- exports ------------------------------------------------------------------------------

#[test]
fn an_export_is_public_effect_free_scalar_and_not_generic() {
    let ok = "pub extern \"C\" fn f(x: Int, b: Bool) -> Bool ! {} = \"nz_f\" { b && x > 0 }\n\
              fn main() -> Int { if f(1, true) { 0 } else { 1 } }\n";
    assert_eq!(check_codes(ok), Vec::<String>::new());
    for (src, why) in [
        (
            "extern \"C\" fn f(x: Int) -> Int ! {} = \"nz_f\" { x }\n",
            "not public",
        ),
        (
            "pub extern \"C\" fn f(x: Int) -> Int = \"nz_f\" { x }\n",
            "no effect set",
        ),
        (
            "pub extern \"C\" fn f(x: Int) -> Int ! { io } = \"nz_f\" { x }\n",
            "an effect",
        ),
        (
            "pub extern \"C\" fn f[T](x: T) -> T ! {} = \"nz_f\" { x }\n",
            "generic",
        ),
        (
            "pub extern \"C\" fn f(s: Str) -> Int ! {} = \"nz_f\" { str_len(s) }\n",
            "a Str parameter",
        ),
        (
            "pub extern \"C\" fn f(x: Int) -> Str ! {} = \"nz_f\" { int_to_str(x) }\n",
            "a Str result",
        ),
    ] {
        let codes = check_codes(&format!("{src}fn main() -> Int {{ 0 }}\n"));
        assert!(codes.contains(&"N0391".to_owned()), "{why}: {codes:?}");
    }
    // The symbol rules are the foreign declaration's.
    assert_eq!(
        check_codes(
            "pub extern \"C\" fn f(x: Int) -> Int ! {} = \"9f\" { x }\nfn main() -> Int { 0 }\n"
        ),
        ["N0382"]
    );
    assert_eq!(
        check_codes(
            "pub extern \"C\" fn f(x: Int) -> Int ! {} = \"malloc\" { x }\nfn main() -> Int { 0 }\n"
        ),
        ["N0383"]
    );
}

#[test]
fn an_export_symbol_is_defined_once_in_a_program() {
    let d = Dir::new(&[(
        "prog.nz",
        "pub extern \"C\" fn f(x: Int) -> Int ! {} = \"nz_same\" { x }\n\
         pub extern \"C\" fn g(x: Int) -> Int ! {} = \"nz_same\" { x + 1 }\n\
         fn main() -> Int { f(1) + g(1) }\n",
    )]);
    let o = d.nazm(&["build", "--json", "prog.nz", "-o", "prog"]);
    assert!(!o.status.success());
    assert_eq!(codes(&o), ["N0383"], "{}", stderr(&o));
}

#[test]
fn an_export_is_an_ordinary_function_to_nazm_and_to_the_interpreter() {
    let d = Dir::new(&[(
        "prog.nz",
        "pub extern \"C\" fn triple(x: Int) -> Int ! {} = \"nz_triple\" { x * 3 }\n\
         fn main(io: IoCap) -> Int ! { io } { print(int_to_str(triple(14))); 0 }\n",
    )]);
    let r = d.nazm(&["run", "prog.nz"]);
    assert_eq!(
        String::from_utf8_lossy(&r.stdout),
        "420\n",
        "{}",
        stderr(&r)
    );
    for backend in ["llvm", "cranelift"] {
        let b = d.nazm(&["build", "--backend", backend, "prog.nz", "-o", backend]);
        assert!(b.status.success(), "{}", stderr(&b));
        assert_eq!(run(&d.path(backend), &[]).stdout, "420\n");
    }
    // And the formatter keeps the form.
    let f = d.nazm(&["fmt", "--check", "prog.nz"]);
    assert!(f.status.success(), "{}", stderr(&f));
}

// --- libraries -----------------------------------------------------------------------------

const LIB: &str = "fn square(x: Int) -> Int ! {} { x * x }\n\n\
                   pub extern \"C\" fn sum_squares(a: Int, b: Int) -> Int ! {} = \"nazm_sum_squares\" {\n    \
                   square(a) + square(b)\n}\n\n\
                   pub extern \"C\" fn is_even(n: Int) -> Bool ! {} = \"nazm_is_even\" { n % 2 == 0 }\n\n\
                   pub extern \"C\" fn wrap(n: Int) -> Int ! {} = \"nazm_wrap\" { n * 9223372036854775807 }\n";

const HOST: &str = r#"
#include <stdio.h>
#include "libnz.h"
int main(int argc, char **argv) {
    printf("%lld %d %d\n", (long long)nazm_sum_squares(3, 4), nazm_is_even(10), nazm_is_even(7));
    fflush(stdout);
    if (argc > 1) { printf("%lld\n", (long long)nazm_wrap(2)); printf("not reached\n"); }
    return 0;
}
"#;

const HEADER: &str = "/* Generated by nazm build --lib. Do not edit. */\n\
                      #ifndef NAZM_LIBNZ_H\n#define NAZM_LIBNZ_H\n\n\
                      #include <stdbool.h>\n#include <stdint.h>\n\n\
                      #ifdef __cplusplus\nextern \"C\" {\n#endif\n\n\
                      /* A Nazm failure inside any of these is reported on standard error and ends the\n   \
                      process with status 2: nothing unwinds into C. */\n\
                      bool nazm_is_even(int64_t a0);\n\
                      int64_t nazm_sum_squares(int64_t a0, int64_t a1);\n\
                      int64_t nazm_wrap(int64_t a0);\n\n\
                      #ifdef __cplusplus\n}\n#endif\n\n#endif\n";

#[test]
fn a_library_and_its_header_link_into_a_c_program_under_both_backends() {
    for backend in ["llvm", "cranelift"] {
        let d = Dir::new(&[("lib.nz", LIB), ("main.c", HOST)]);
        let b = d.nazm(&[
            "build",
            "--lib",
            "--no-cache",
            "--backend",
            backend,
            "lib.nz",
            "-o",
            "libnz.a",
        ]);
        assert!(b.status.success(), "{backend}: {}", stderr(&b));
        assert_eq!(
            std::fs::read_to_string(d.path("libnz.h")).unwrap(),
            HEADER,
            "{backend}"
        );
        d.clang(&["main.c", "libnz.a", "-o", "host"]);
        let r = run(&d.path("host"), &[]);
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "25 1 0\n"),
            "{backend}: {r:?}"
        );
        // A failure inside an export ends the process with Nazm's diagnostic and status 2,
        // and C never sees a value.
        let r = run(&d.path("host"), &["wrap"]);
        assert_eq!(r.code, Some(2), "{backend}: {r:?}");
        assert_eq!(r.stdout, "25 1 0\n", "{backend}");
        assert!(r.stderr.contains("N0400"), "{backend}: {r:?}");
    }
}

#[test]
fn a_library_is_the_same_bytes_on_every_build_and_needs_no_main() {
    let d = Dir::new(&[("lib.nz", LIB)]);
    let mut seen = Vec::new();
    for out in ["one.a", "two.a"] {
        let b = d.nazm(&["build", "--lib", "--no-cache", "lib.nz", "-o", out]);
        assert!(b.status.success(), "{}", stderr(&b));
        seen.push(std::fs::read(d.path(out)).unwrap());
    }
    assert_eq!(seen[0], seen[1], "two builds of one library differ");
    // Without `--lib`, a program with no `main` is still refused.
    let o = d.nazm(&["build", "lib.nz", "-o", "exe"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("no `main`"), "{}", stderr(&o));
}
