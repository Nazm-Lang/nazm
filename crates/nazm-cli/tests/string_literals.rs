//! R1a: multi-line and raw string literals (`docs/spec.md`, *Multi-line and raw strings*).
//!
//! Every implementation is asked for the bytes the rules give, worked out by hand; the formatter
//! keeps a literal as written; and what the lexer refuses, by which code, once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
            label.to_owned(),
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

fn nothing_live(r: &Ran) -> bool {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .is_some_and(|l| {
            l.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0")
        })
}

/// Each literal printed between `[` and `]`, so every byte the rules keep or drop is visible.
const STRINGS: &str = r###"fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat("[", str_concat(s, "]\n"))) }

fn main(out: OutCap) -> Int ! {io} {
    // The closing delimiter's indentation comes off every line; the first and last breaks are
    // not the value's; escapes are an ordinary string's; a backslash ending a line joins it.
    let ir = """
        define i64 @f() {
          ret i64 "\x41"
        }
          two\
        joined
        """;
    show(out, ir);
    // A blank line inside is a line, whatever indentation it has.
    show(out, """
      a

      b
      """);
    // Raw: no escapes, quotes with `#`s, line breaks as written.
    show(out, r"C:\path\n");
    show(out, r#"say "hi" \n"#);
    show(out, r##"a "# b"##);
    show(out, r"two
lines");
    show(out, """
    """);
    0
}
"###;

const EXPECTED: &str = "[define i64 @f() {\n  ret i64 \"A\"\n}\n  twojoined]\n[a\n\nb]\n[C:\\path\\n]\n[say \"hi\" \\n]\n[a \"# b]\n[two\nlines]\n[]\n";

#[test]
fn multi_line_and_raw_strings_are_the_same_bytes_everywhere() {
    for (label, r) in everywhere(STRINGS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {:?}", r.stdout);
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

#[test]
fn the_formatter_keeps_a_literal_as_written() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f.nz");
    std::fs::write(&p, STRINGS).unwrap();
    let r = run_bounded(Command::new(NAZM).arg("fmt").arg(&p), "nazm fmt");
    assert_eq!(r.code, Some(0), "{r:?}");
    let got = std::fs::read_to_string(&p).unwrap();
    for literal in [
        "\"\"\"\n        define i64 @f() {\n          ret i64 \"\\x41\"\n        }\n          two\\\n        joined\n        \"\"\"",
        "r#\"say \"hi\" \\n\"#",
        "r\"two\nlines\"",
    ] {
        assert!(got.contains(literal), "{literal:?} in {got}");
    }
}

/// `nazm check` on `src`: every error code, in order, and the first message.
fn refused(src: &str) -> (Vec<String>, String) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    let codes = r
        .stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect();
    let first = r
        .stderr
        .lines()
        .find(|l| l.starts_with("error["))
        .unwrap_or("")
        .to_owned();
    (codes, first)
}

#[test]
fn what_a_literal_refuses_and_by_which_code() {
    for (src, code, says) in [
        (
            "fn main() -> Int { let s = \"\"\"x\n  \"\"\"; 0 }\n",
            "N0005",
            "begins on the line after",
        ),
        (
            "fn main() -> Int { let s = \"\"\"\n  a\n b\n  \"\"\"; 0 }\n",
            "N0005",
            "does not begin with its closing",
        ),
        (
            "fn main() -> Int { let s = \"\"\"\n  \\q\n  \"\"\"; 0 }\n",
            "N0004",
            "",
        ),
        (
            "fn main() -> Int { let s = r#\"a\"; 0 }\nfn f() -> Int { 1 }\n",
            "N0002",
            "no closing quote",
        ),
    ] {
        let (codes, first) = refused(src);
        assert_eq!(
            codes.first().map(String::as_str),
            Some(code),
            "{src}: {codes:?}"
        );
        assert!(first.contains(says), "{src}: {first}");
    }
    // A name `r` is still a name.
    let (codes, _) = refused("fn main() -> Int { let r = 1; r }\n");
    assert!(codes.is_empty(), "{codes:?}");
}
