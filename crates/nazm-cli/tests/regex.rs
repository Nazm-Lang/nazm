//! R1e, S5: `@std/regex` (`docs/spec.md`, *The standard library*).
//!
//! A table of patterns and texts, each pattern's every match as `regex_find_all` finds them —
//! leftmost-first, as Rust's `regex` and Perl find them. Every expectation in the first part of the
//! table was checked, when it was written, against Python's `re` searching from each position as
//! Rust's `find_iter` does (they agree there); the Unicode classes and the flags Python spells
//! otherwise were checked by hand. Capture groups; every malformed pattern refused with the byte
//! offset where; patterns that take exponential time in a backtracking matcher finishing at once on
//! long texts; and a tokeniser's pre-split, captures and errors alike under the interpreter, the
//! LIR oracle, LLVM `-O0`/`-O2` and Cranelift, with nothing live.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// `s` as a Nazm string literal.
fn lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 || c as u32 == 127 => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const HEAD: &str = r#####"use "@std/regex";

fn spans(ms: Vec[Match]) -> Str {
    let mut out = "";
    for i in 0..vec_len(ms) {
        let m = vec_get(ms, i);
        out = f"{out}({m.start},{m.end})";
    }
    out
}

fn all(pattern: Str, text: Str) -> Str {
    match regex_compile(pattern) {
        Result.Ok(value: r) => spans(regex_find_all(r, text)),
        Result.Err(error: e) => f"error: {e}",
    }
}

fn groups(pattern: Str, text: Str) -> Str {
    match regex_compile(pattern) {
        Result.Ok(value: r) => match regex_captures(r, text, 0) {
            Option.Some(value: gs) => {
                let mut out = "";
                for i in 0..vec_len(gs) {
                    let g = match vec_get(gs, i) {
                        Option.Some(value: m) => f"({m.start},{m.end})",
                        Option.None() => "-",
                    };
                    out = f"{out}{g}";
                }
                out
            },
            Option.None() => "no match",
        },
        Result.Err(error: e) => f"error: {e}",
    }
}

"#####;

/// A program printing `f(pattern, text)` for each case, a line each.
fn program(f: &str, cases: &[(&str, &str)]) -> String {
    let mut src = String::from(HEAD);
    src.push_str("fn main(io: IoCap) -> Int ! {io} {\n");
    for (p, t) in cases {
        let _ = writeln!(
            src,
            "    print(str_concat({f}({}, {}), \"\\n\"));",
            lit(p),
            lit(t)
        );
    }
    src.push_str("    0\n}\n");
    src
}

/// Each case's line, under the interpreter.
fn lines(f: &str, cases: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), program(f, cases)).unwrap();
    let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    r.stdout.lines().map(str::to_owned).collect()
}

fn check(f: &str, table: &[(&str, &str, &str)]) {
    let cases: Vec<(&str, &str)> = table.iter().map(|(p, t, _)| (*p, *t)).collect();
    let got = lines(f, &cases);
    for (i, (p, t, want)) in table.iter().enumerate() {
        assert_eq!(
            got.get(i).map(String::as_str),
            Some(*want),
            "{p:?} on {t:?}"
        );
    }
}

/// The GPT-2 pre-tokeniser's pattern, and `cl100k_base`'s.
const GPT2: &str =
    r"(?i:'s|'t|'re|'ve|'m|'ll|'d)| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+";
const CL100K: &str = r"(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}{1,3}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+";

#[test]
fn every_pattern_finds_its_leftmost_first_matches() {
    check(
        "all",
        &[
            // Literals, alternation, leftmost-first preference.
            ("abc", "xabcabc", "(1,4)(4,7)"),
            ("a|ab", "ab", "(0,1)"),
            ("ab|a", "ab", "(0,2)"),
            ("(a|ab)(c|bcd)(d*)", "abcd", "(0,4)"),
            ("(ab|a)(bc|c)?", "abc", "(0,3)"),
            // Empty matches: none right where the one before ended.
            ("a*", "baaab", "(0,0)(1,4)(5,5)"),
            ("a|", "ba", "(0,0)(1,2)"),
            ("", "ab", "(0,0)(1,1)(2,2)"),
            // Repetition, greedy and lazy.
            ("a+?", "aaa", "(0,1)(1,2)(2,3)"),
            ("a*?b", "aaab", "(0,4)"),
            ("a{2,3}", "aaaaaaa", "(0,3)(3,6)"),
            ("a{2,3}?", "aaaaa", "(0,2)(2,4)"),
            ("a{2}", "aaaaa", "(0,2)(2,4)"),
            ("a{2,}", "a aa aaaa", "(2,4)(5,9)"),
            ("a{0,2}?b", "aab", "(0,3)"),
            ("x{0}y", "xy", "(1,2)"),
            ("x?y", "xyy", "(0,2)(2,3)"),
            ("(a|b)*c", "abacbc", "(0,4)(4,6)"),
            ("(?:ab)+", "ababab abab", "(0,6)(7,11)"),
            ("<.+?>", "<a><b>", "(0,3)(3,6)"),
            ("<.+>", "<a><b>", "(0,6)"),
            // Classes.
            ("[a-c]+", "xxabcbay", "(2,7)"),
            ("[^a-c]+", "abxyzc", "(2,5)"),
            ("[-a]+", "a-a-b", "(0,4)"),
            ("[a-]+", "a-a-b", "(0,4)"),
            ("[]a]+", "]a]b", "(0,3)"),
            ("[^]a]+", "]a]bc", "(3,5)"),
            (r"[\d\s]+", "a1 2b", "(1,4)"),
            (r"[\]\\]+", "x]\\]y", "(1,4)"),
            (r"\d+", "a12b345", "(1,3)(4,7)"),
            (r"\D+", "a12b345", "(0,1)(3,4)"),
            (r"\w+", "foo_bar baz-qux", "(0,7)(8,11)(12,15)"),
            (r"\W+", "foo_bar baz-qux", "(7,8)(11,12)"),
            (r"\s+", "a \t\nb\r\x0b\x0cc", "(1,4)(5,8)"),
            (r"\S+", "a \t\nbc", "(0,1)(4,6)"),
            (r"[^\n]+", "ab\ncd", "(0,2)(3,5)"),
            // Anchors and boundaries.
            ("^a", "aaa", "(0,1)"),
            ("a$", "aaa", "(2,3)"),
            ("^$", "", "(0,0)"),
            (r"(?m)^\w+$", "ab\ncd\n", "(0,2)(3,5)"),
            ("(?m)$", "a\nb", "(1,1)(3,3)"),
            (r"\bfoo\b", "foo foobar barfoo foo", "(0,3)(18,21)"),
            (r"\Bo\B", "foo boo", "(1,2)(5,6)"),
            (r"\Afoo", "foofoo", "(0,3)"),
            (r"foo\z", "foofoo", "(3,6)"),
            // `.` and UTF-8: one whole scalar value.
            (".", "héllo", "(0,1)(1,3)(3,4)(4,5)(5,6)"),
            (".+", "a\nb", "(0,1)(2,3)"),
            ("(?s).+", "a\nb", "(0,3)"),
            ("(?s:.)b", "\nb", "(0,2)"),
            ("é+", "aéééb", "(1,7)"),
            ("[é-ë]+", "aéêëb", "(1,7)"),
            // Flags.
            ("(?i)hello", "HeLLo hello", "(0,5)(6,11)"),
            ("(?i)[a-c]+", "xABCcbx", "(1,6)"),
            ("(?i)[^a]", "aAb", "(2,3)"),
            ("(?i:a)b", "Ab AB", "(0,2)"),
            ("a(?i)b|c", "aB C", "(0,2)(3,4)"),
            ("(?i)a(?-i)b", "AB Ab ab", "(3,5)(6,8)"),
            // Escapes.
            (r"\x41\u{42}\x{43}\u0044", "ABCD", "(0,4)"),
            (r"a\.b", "a.b axb", "(0,3)"),
            (r"\(\)\[\]\{\}\*\+\?\|\^\$", "()[]{}*+?|^$", "(0,12)"),
            (r"\t\n", "x\t\ny", "(1,3)"),
            ("}", "}}", "(0,1)(1,2)"),
            ("]", "]", "(0,1)"),
            // One-scalar lookahead.
            (r"\s+(?!\S)|\s+", "a   b  ", "(1,3)(3,4)(5,7)"),
            (r"(?=a)\w", "ba", "(1,2)"),
            ("a(?!b)", "ab ac a", "(3,4)(6,7)"),
            // Unicode classes (checked by hand).
            (r"\p{L}+", "héllo wörld 123", "(0,6)(7,13)"),
            (r"\p{Lu}\p{Ll}+", "Hello World", "(0,5)(6,11)"),
            (r"\pN+", "x\u{663}\u{664}5\u{bd}", "(1,8)"),
            (r"\P{L}+", "ab12cd", "(2,4)"),
            (r"[^\p{L}\p{N}\s]+", "hi, there! 42", "(2,3)(9,10)"),
            (r"\p{M}", "e\u{301}", "(1,3)"),
            (r"\d", "\u{663}", "(0,2)"),
            (r"\w+", "naïve_日本", "(0,13)"),
            (r"\s", "\u{3000}\u{a0}", "(0,3)(3,5)"),
            // Pre-tokenisers.
            (
                GPT2,
                "Hello's world  42!!  I'LL",
                "(0,5)(5,7)(7,13)(13,14)(14,17)(17,19)(19,20)(20,22)(22,25)",
            ),
            (
                CL100K,
                "It's 12345 cafés...\n\n  ok",
                "(0,2)(2,4)(4,5)(5,8)(8,10)(10,17)(17,22)(22,23)(23,26)",
            ),
        ],
    );
}

#[test]
fn capture_groups_report_what_each_matched() {
    check(
        "groups",
        &[
            ("(a)(b)?", "a", "(0,1)(0,1)-"),
            ("(a|ab)(c|bcd)(d*)", "abcd", "(0,4)(0,1)(1,4)(4,4)"),
            (
                r"(\w+)@(\w+)\.com",
                "mail bob@example.com now",
                "(5,20)(5,8)(9,16)",
            ),
            ("(?:(a)|b)+", "ab", "(0,2)(0,1)"),
            (
                r"(?<y>\d{4})-(?P<m>\d{2})",
                "on 2026-10 ok",
                "(3,10)(3,7)(8,10)",
            ),
            ("(a*)+", "b", "(0,0)(0,0)"),
            ("(?:x)(y)", "zxy", "(1,3)(2,3)"),
            ("(é)(.)", "aéb", "(1,4)(1,3)(3,4)"),
            ("x(y)", "abc", "no match"),
        ],
    );
}

#[test]
fn every_malformed_pattern_is_refused_with_its_offset() {
    let want = |w: &str| format!("error: {w}");
    let table: Vec<(&str, String)> = vec![
        ("(ab", want("an unclosed group at byte 0")),
        ("ab)", want("an unmatched `)` at byte 2")),
        ("x[ab", want("an unclosed class at byte 1")),
        (
            "a{2,1}",
            want("a counted repetition whose most is below its least at byte 1"),
        ),
        ("a{", want("a counted repetition needs a count at byte 1")),
        ("a{2", want("an unclosed counted repetition at byte 1")),
        ("*a", want("a repetition with nothing to repeat at byte 0")),
        ("a|+", want("a repetition with nothing to repeat at byte 2")),
        ("a**", want("a repetition of a repetition at byte 2")),
        (r"(a)\1", want("backreferences are not supported at byte 3")),
        ("(?<=a)b", want("lookbehind is not supported at byte 0")),
        (
            "(?=ab)",
            want("a lookahead holds one character or class at byte 0"),
        ),
        (
            r"\p{Greek}",
            want("an unknown Unicode class `Greek` at byte 0"),
        ),
        (
            "[z-a]",
            want("a range whose end is before its start at byte 1"),
        ),
        (r"[\w-z]", want("a class cannot begin a range at byte 3")),
        (r"[a-\d]", want("a class cannot end a range at byte 3")),
        (r"a\q", want("an unknown escape `\\q` at byte 1")),
        ("(?x)a", want("an unknown group flag at byte 2")),
        ("a\\", want("a `\\` at the end of the pattern at byte 1")),
        (
            "[[:alpha:]]",
            want("POSIX classes are not supported at byte 1"),
        ),
        (
            "[a[b]]",
            want("a `[` inside a class must be escaped at byte 2"),
        ),
        (
            "[a&&b]",
            want("class set operations are not supported at byte 2"),
        ),
        (
            r"\x{110000}",
            want("an escape that is not a Unicode scalar value at byte 0"),
        ),
        (r"\xZ1", want("an escape needs 2 hex digits at byte 0")),
        (
            "(a{1000}){1000}",
            want("a repetition too large once written out at byte 9"),
        ),
        (
            "(?P<>a)",
            want("a group name is letters, digits and `_`, then `>` at byte 0"),
        ),
    ];
    let cases: Vec<(&str, &str)> = table.iter().map(|(p, _)| (*p, "")).collect();
    let got = lines("all", &cases);
    for (i, (p, w)) in table.iter().enumerate() {
        assert_eq!(got.get(i), Some(w), "{p:?}");
    }
}

const PATHOLOGICAL: &str = r#####"use "@std/regex";

fn probe(p: Str, text: Str) -> Str {
    match regex_compile(p) {
        Result.Ok(value: r) => {
            let found = match regex_find(r, text, 0) {
                Option.Some(value: m) => f"({m.start},{m.end})",
                Option.None() => "none",
            };
            f"{p}: {regex_is_match(r, text)} {found}\n"
        },
        Result.Err(error: e) => f"{p}: {e}\n",
    }
}

fn main(io: IoCap) -> Int ! {io} {
    let n = 40;
    let mut text = "";
    for i in 0..n { text = str_concat(text, "a"); }
    print(probe("(a*)*b", text));
    print(probe("(a|aa)*c", text));
    print(probe("(a+a+)+b", text));
    print(probe("(a?){40}a{40}b", text));
    print(probe("(?:a*)*$x", text));
    match regex_compile("(a*)*b") {
        Result.Ok(value: r) => {
            let ms = regex_find_all(r, str_concat(text, "b"));
            print(f"{vec_len(ms)} {vec_get(ms, 0).start} {vec_get(ms, 0).end}\n");
        },
        Result.Err(error: e) => print(e),
    };
    0
}
"#####;

/// What a backtracking matcher takes 2^40 steps or more for, on texts of 40 `a`s and of 200,000.
#[test]
fn pathological_patterns_take_linear_time() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PATHOLOGICAL).unwrap();
    let want = |n: usize| {
        format!(
            "(a*)*b: false none\n(a|aa)*c: false none\n(a+a+)+b: false none\n\
             (a?){{40}}a{{40}}b: false none\n(?:a*)*$x: false none\n1 0 {}\n0\n",
            n + 1
        )
    };
    let started = std::time::Instant::now();
    let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(r.stdout, want(40));
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    // 200,000 `a`s, compiled.
    std::fs::write(
        d.path().join("prog.nz"),
        PATHOLOGICAL.replace("let n = 40;", "let n = 200000;"),
    )
    .unwrap();
    let exe = d.path().join(crate::common::exe_name("long"));
    let b = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--backend",
            "llvm",
            "--opt-level",
            "2",
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ],
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let r = common::run_within(
        common::bounded_exe(&exe).current_dir(d.path()),
        std::time::Duration::from_secs(30),
    )
    .expect("200,000 bytes searched within 30 seconds");
    assert_eq!(r.code, Some(0), "{r:?}");
    assert_eq!(r.stdout, want(200_000));
}

const PROGRAM: &str = r#####"use "@std/regex";

fn spans(ms: Vec[Match]) -> Str {
    let mut out = "";
    for i in 0..vec_len(ms) {
        let m = vec_get(ms, i);
        out = f"{out}({m.start},{m.end})";
    }
    out
}

fn main(io: IoCap) -> Int ! {io} {
    let text = "It's 12345 cafés...\n\n  OK, naïve 日本語!";
    match regex_compile("(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\\r\\n\\p{L}\\p{N}]?\\p{L}+|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n]*|\\s*[\\r\\n]+|\\s+(?!\\S)|\\s+") {
        Result.Ok(value: r) => {
            let ms = regex_find_all(r, text);
            let pieces = strs_new();
            for i in 0..vec_len(ms) {
                let m = vec_get(ms, i);
                strs_push(pieces, str_slice(text, m.start, m.end));
            }
            print(f"{spans(ms)}\n{str_join(pieces, "|")}\n");
        },
        Result.Err(error: e) => print(f"error: {e}\n"),
    };
    match regex_compile("(\\w+)@(\\w+)(?:\\.(com|org))?") {
        Result.Ok(value: r) => {
            print(f"groups: {regex_groups(r)}\n");
            match regex_captures(r, "to: bob@example.org, amy@home", 0) {
                Option.Some(value: gs) => {
                    for i in 0..vec_len(gs) {
                        match vec_get(gs, i) {
                            Option.Some(value: m) => print(f"{i}: {m.start}..{m.end}\n"),
                            Option.None() => print(f"{i}: -\n"),
                        };
                    }
                },
                Option.None() => print("no match\n"),
            };
            match regex_find(r, "to: bob@example.org, amy@home", 20) {
                Option.Some(value: m) => print(f"from 20: {m.start}..{m.end}\n"),
                Option.None() => print("from 20: none\n"),
            };
            print(f"{regex_is_match(r, "nobody")} {regex_is_match(r, "a@b")}\n");
        },
        Result.Err(error: e) => print(f"error: {e}\n"),
    };
    match regex_compile("[a-") {
        Result.Ok(value: r) => print("compiled\n"),
        Result.Err(error: e) => print(f"error: {e}\n"),
    };
    0
}
"#####;

const EXPECTED: &str =
    "(0,2)(2,4)(4,5)(5,8)(8,10)(10,17)(17,22)(22,23)(23,26)(26,27)(27,34)(34,44)(44,45)
It|'s| |123|45| cafés|...\n\n| | OK|,| naïve| 日本語|!
groups: 3
0: 4..19
1: 4..7
2: 8..15
3: 16..19
from 20: 21..29
false true
error: an unclosed class at byte 0
0
";

#[test]
fn a_tokeniser_split_agrees_everywhere() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let mut runs = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        runs.push((
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
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        runs.push((
            label.to_owned(),
            run_bounded(
                common::bounded_exe(&exe)
                    .current_dir(d.path())
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    for (label, r) in runs {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert_eq!(r.stdout, EXPECTED, "{label}");
        if label != "lir oracle" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {r:?}"));
            assert!(
                line.split_whitespace()
                    .filter(|w| w.starts_with("live="))
                    .all(|w| w == "live=0"),
                "{label}: {line}"
            );
        }
    }
}
