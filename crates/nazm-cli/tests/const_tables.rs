//! R1a: arrays of strings, constant tables and exported constants (`docs/spec.md`, *Arrays* and
//! *Constant tables and exported constants*).
//!
//! Every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift
//! — is asked for what the rules work out by hand, with every string reclaimed; then a constant
//! crossing a module's interface, plainly and under a name, fresh and from the cache; then what is
//! refused, by which code.

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

const ARRAYS: &str = "struct Kw { name: Str, code: Int, }
struct Holder { words: [Str; 2], n: Int, }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }
fn pick(xs: [Str; 3], i: Int) -> Str { xs[i] }

fn main(out: OutCap) -> Int ! {io} {
    // Built from literals and from strings made at run time; copied, then written.
    let names = [\"alpha\", \"beta\", str_concat(\"gam\", \"ma\")];
    let mut copy = names;
    copy[0] = str_concat(\"del\", \"ta\");
    show(out, str_concat(names[0], copy[0]));
    show(out, pick(names, 2));
    // Records holding strings, in an array; an array in a record; arrays of arrays.
    let kws = [Kw(name: \"if\", code: 1), Kw(name: str_concat(\"wh\", \"ile\"), code: 2)];
    let mut t = 0;
    let mut i = 0;
    while i < 2 { t = t + str_len(kws[i].name) * kws[i].code; i = i + 1; }
    show(out, num_to_str(t));
    let mut h = Holder(words: [str_concat(\"x\", \"y\"), \"z\"], n: 1);
    h.words[1] = str_concat(\"q\", \"r\");
    show(out, str_concat(h.words[0], h.words[1]));
    let grid = [[str_concat(\"a\", \"\"); 2]; 2];
    show(out, grid[1][1]);
    // In a `Vec`, compared, given to a task.
    let v = vec_new[[Str; 3]]();
    vec_push(v, names);
    vec_push(v, copy);
    show(out, vec_get(v, 1)[0]);
    if names == [\"alpha\", \"beta\", \"gamma\"] && names != copy { show(out, \"eq\"); }
    0
}
";

/// By hand: `while` is 5 letters, `if` 2, so 2·1 + 5·2.
const ARRAYS_EXPECTED: &str = "alphadelta\ngamma\n12\nxyqr\na\ndelta\neq\n";

#[test]
fn arrays_of_strings_mean_the_same_everywhere_and_are_reclaimed() {
    for (label, r) in everywhere(ARRAYS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout.starts_with(ARRAYS_EXPECTED),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

const TABLES: &str = "struct Keyword { text: Str, code: Int, }
enum Color { Red, Rgb(r: UInt8, g: UInt8, b: UInt8), }
const N: Int = 3;
const KEYWORDS: [Keyword; N] = [
    Keyword(text: \"fn\", code: 1),
    Keyword(code: N - 1, text: \"let\"),
    Keyword(text: \"match\", code: N),
];
const ORIGIN: (Int, Int) = (0, -1);
const TEAL: Color = Color.Rgb(r: 0, g: 128, b: UInt8(64) * 2);
const ZEROS: [Int; 4] = [0; 4];
const TABLE: [[Str; 2]; 2] = [[\"a\", \"b\"], [\"c\", \"d\"]];
const COPY: [Keyword; 3] = KEYWORDS;

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

fn code_of(word: Str) -> Int {
    let mut i = 0;
    while i < N {
        if KEYWORDS[i].text == word { return KEYWORDS[i].code; }
        i = i + 1;
    }
    ORIGIN.1
}

fn main(out: OutCap) -> Int ! {io} {
    show(out, num_to_str(code_of(\"let\") * 100 + code_of(\"match\") * 10 + code_of(\"zz\")));
    let b = match TEAL { Color.Rgb(r: _, g: _, b: b) => Int(b), Color.Red() => 0, };
    show(out, num_to_str(b + ZEROS[3]));
    show(out, str_concat(TABLE[1][0], COPY[0].text));
    // A use is a copy: writing one changes no other.
    let mut mine = KEYWORDS;
    mine[0] = Keyword(text: str_concat(\"f\", \"un\"), code: 9);
    show(out, str_concat(mine[0].text, KEYWORDS[0].text));
    0
}
";

const TABLES_EXPECTED: &str = "229\n128\ncfn\nfunfn\n";

#[test]
fn constant_tables_mean_the_same_everywhere_and_are_reclaimed() {
    for (label, r) in everywhere(TABLES) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout.starts_with(TABLES_EXPECTED),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

const LIB: &str = "pub struct Keyword { text: Str, code: Int, }
pub const N: Int = 2;
pub const KEYWORDS: [Keyword; N] = [Keyword(text: \"fn\", code: 1), Keyword(text: \"let\", code: 2)];
pub const GREETING: Str = \"hi\";
";

fn in_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    d
}

fn run_in(d: &tempfile::TempDir, file: &str) -> Ran {
    run_bounded(
        Command::new(NAZM).current_dir(d.path()).args(["run", file]),
        "nazm run",
    )
}

fn build_in(d: &tempfile::TempDir, file: &str) -> Ran {
    let exe = d.path().join("prog");
    let b = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["build", file, "-o", exe.to_str().unwrap()]),
        "nazm build",
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    run_bounded(&mut common::bounded_exe(&exe), "built")
}

#[test]
fn an_exported_constant_crosses_a_module_plainly_and_under_a_name() {
    let d = in_dir(&[
        ("lib.nz", LIB),
        (
            "main.nz",
            "use \"lib.nz\";\nconst DOUBLE: Int = N * 2;\n\
             const FIRST: [Keyword; 1] = [Keyword(text: GREETING, code: DOUBLE)];\n\
             fn main() -> Int { KEYWORDS[1].code * 100 + FIRST[0].code * 10 + str_len(GREETING) }\n",
        ),
        (
            "q.nz",
            "use \"lib.nz\" as lib;\nfn main() -> Int { lib::KEYWORDS[1].code * 10 + lib::N }\n",
        ),
    ]);
    for (file, want) in [("main.nz", "242\n"), ("q.nz", "22\n")] {
        let r = run_in(&d, file);
        assert!(r.stdout.starts_with(want), "{file}: {r:?}");
        // Twice built: the second reads the library's interface from the cache.
        for _ in 0..2 {
            let r = build_in(&d, file);
            assert_eq!(r.code, Some(0), "{file}: {r:?}");
        }
    }
}

#[test]
fn an_exported_constant_is_in_the_published_interface() {
    let d = in_dir(&[("lib.nz", LIB)]);
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["interface", "lib.nz"]),
        "nazm interface",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["schema"], "nazm.interface/12");
    let names: Vec<&str> = v["consts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["GREETING", "KEYWORDS", "N"]);
    assert_eq!(v["consts"][0]["value"]["bytes"], "6869");
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
fn what_a_constant_table_refuses_and_by_which_code() {
    for (src, code, says) in [
        (
            "struct P { x: Int, }\nconst A: P = P(x: 1, y: 2);\nfn main() -> Int { 0 }",
            "N0300",
            "no field `y`",
        ),
        (
            "struct P { x: Int, }\nconst A: [P; 2] = [P(x: 1)];\nfn main() -> Int { 0 }",
            "N0300",
            "[P; 2]",
        ),
        (
            "struct P { x: UInt8, }\nconst A: P = P(x: 300);\nfn main() -> Int { 0 }",
            "N0003",
            "",
        ),
        (
            "struct P { x: Int, }\nconst A: P = P(x: 1 / 0);\nfn main() -> Int { 0 }",
            "N0623",
            "divides by zero",
        ),
        (
            "struct P { x: Int, }\nfn f() -> Int { 1 }\nconst A: P = P(x: f());\nfn main() -> Int { 0 }",
            "N0622",
            "",
        ),
        (
            "struct R { v: Vec[Int], }\nconst A: R = R(v: vec_new[Int]());\nfn main() -> Int { 0 }",
            "N0622",
            "holds a `Vec[Int]`",
        ),
        (
            "struct P { x: Int, }\nconst A: P = P(x: 1);\nconst B: Int = A;\nfn main() -> Int { 0 }",
            "N0622",
            "not a number",
        ),
        (
            "struct P { x: Int, }\nconst A: P = B;\nconst B: P = A;\nfn main() -> Int { 0 }",
            "N0622",
            "depends on itself",
        ),
        // A handle is no array's element; a static is not exported.
        (
            "fn main() -> Int { let a = [vec_new[Int]()]; 0 }",
            "N0621",
            "is a handle",
        ),
        (
            "pub static S: Int = 1;\nfn main() -> Int { 0 }",
            "N0101",
            "a static is its own module's",
        ),
        // A public constant may not expose a private type.
        (
            "struct P { x: Int, }\npub const A: [P; 1] = [P(x: 1)];\nfn main() -> Int { 0 }",
            "N0337",
            "exposes `P`",
        ),
    ] {
        let (codes, first) = refused(src);
        assert_eq!(codes.first().map(String::as_str), Some(code), "{src}: {codes:?}");
        assert_eq!(codes.len(), 1, "{src}: one mistake, one error: {codes:?}");
        assert!(first.contains(says), "{src}: {first}");
    }
}
