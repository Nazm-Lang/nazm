//! R1a: tuples — `(A, B)` types, `(a, b)` values, `t.0` and patterns that take them apart
//! (`docs/spec.md`, *Tuples*).
//!
//! Every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift
//! — is asked for what the section's rules work out by hand, with every allocation reclaimed;
//! then what the checker refuses, by which code; then a tuple crossing a module's interface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

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

const TUPLES: &str = "struct Entry { key: Str, at: (Int, Int), }
enum Cell { Empty, Full(v: (Str, Int)), }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

// A generic function over a tuple, and a tuple returned.
fn swap[A, B](p: (A, B)) -> (B, A) { let (a, b) = p; (b, a) }
fn div_mod(a: Int, b: Int) -> (Int, Int) { (a / b, a % b) }

// Tuple patterns in a `match`: literals, names and `_` inside, nested.
fn classify(p: ((Int, Int), Str)) -> Str {
    match p {
        ((0, 0), _) => \"origin\",
        ((0, _), name) | ((_, 0), name) => str_concat(\"axis \", name),
        ((x, y), _) => num_to_str(x * y),
    }
}

fn main(out: OutCap) -> Int ! {io} {
    let (q, r) = div_mod(17, 5);
    show(out, num_to_str(q * 10 + r));
    let s = swap((\"left\", 2));
    show(out, str_concat(num_to_str(s.0), s.1));
    let nested = ((1, (2, 3)), \"n\");
    show(out, num_to_str(nested.0.1.1 * 100 + nested.0.0));
    show(out, str_concat(classify(((0, 0), \"a\")), str_concat(classify(((0, 4), \"b\")),
        str_concat(classify(((5, 0), \"c\")), classify(((2, 3), \"d\"))))));
    // Assignment through a mutable binding, element by element.
    let mut m = ((0, 0), \"was\");
    m.0.1 = 7;
    m.1 = \"is\";
    show(out, str_concat(m.1, num_to_str(m.0.1)));
    // A `for` loop that takes each element apart; owning elements released.
    let xs = vec_new[(Str, Int)]();
    vec_push(xs, (\"a\", 1));
    vec_push(xs, (\"bb\", 2));
    let mut total = 0;
    for (k, v) in xs { total = total + str_len(k) * v; }
    show(out, num_to_str(total));
    // In a record's field and an enum's payload; equality is element by element.
    let e = Entry(key: \"k\", at: (3, 4));
    let c = Cell.Full(v: (\"x\", 9));
    if let Cell.Full(v: (_, n)) = c { show(out, num_to_str(n + e.at.1)); }
    if (1, \"a\") == (1, \"a\") && (1, \"a\") != (1, \"b\") { show(out, \"equal\"); }
    0
}
";

/// What the section's rules give, worked out by hand.
const EXPECTED: &str = "32\n2left\n301\noriginaxis baxis c6\nis7\n5\n13\nequal\n";

#[test]
fn tuples_mean_the_same_everywhere_and_are_reclaimed() {
    for (label, r) in everywhere(TUPLES) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
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
fn what_a_tuple_refuses_and_by_which_code() {
    for (src, code, says) in [
        // A tuple of one: `(x)` is `x`, and `(x,)` is refused by name.
        ("let s = (3,); 0", "N0101", "two or more"),
        // A position the tuple does not have.
        ("let t = (1, 2); t.2", "N0333", "no field `2`"),
        // A pattern of another arity, or a tuple pattern against something else.
        ("let (a, b, c) = (1, 2); a", "N0300", "a tuple of 3"),
        ("let (a, b) = 5; a", "N0300", "a tuple of 2"),
        // A tuple of other element types.
        (
            "let t = (1, \"a\"); if t == (1, 2) { 1 } else { 0 }",
            "N0304",
            "",
        ),
        // Equality element by element, where every element has it.
        (
            "let a = (1, vec_new[Int]()); if a == a { 1 } else { 0 }",
            "N0304",
            "no equality",
        ),
        // A `let` without `else` must match every value.
        (
            "let (0, b) = (1, 2); b",
            "N0343",
            "this `let` pattern does not match",
        ),
        // Covering: every combination of the elements' values.
        (
            "match (1, true) { (_, true) => 1, (0, false) => 2, }",
            "N0343",
            "(_, false)",
        ),
        // An element of an immutable binding is not replaced.
        ("let t = (1, 2); t.0 = 5; 0", "N0310", "not mutable"),
        // `for` over a pattern needs elements the pattern matches.
        (
            "let mut s = 0; for (a, b) in 0..3 { s = s + a; } s",
            "N0300",
            "a tuple of 2",
        ),
    ] {
        let (codes, first) = refused(&format!("fn main() -> Int {{ {src} }}\n"));
        assert_eq!(
            codes.first().map(String::as_str),
            Some(code),
            "{src}: {codes:?}"
        );
        assert_eq!(codes.len(), 1, "{src}: one mistake, one error: {codes:?}");
        assert!(first.contains(says), "{src}: {first}");
    }
}

#[test]
fn a_parenthesised_type_value_or_pattern_is_not_a_tuple() {
    let (codes, _) = refused(
        "fn f(x: (Int)) -> (Int) { let (y) = (x); match (y) { (0) => 1, (n) => n, } }\n\
         fn main() -> Int { f(1) }\n",
    );
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn a_tuple_crosses_a_module_interface() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("lib.nz"),
        "pub fn split(s: Str) -> (Int, Str) ! {} { (str_len(s), s) }\n\
         pub fn swap[A, B](p: (A, B)) -> (B, A) ! {} { let (a, b) = p; (b, a) }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("main.nz"),
        "use \"lib.nz\";\n\
         fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }\n\
         fn main(out: OutCap) -> Int ! {io} {\n\
             let (n, s) = split(\"abc\");\n\
             let q = swap((n, s));\n\
             show(out, str_concat(q.0, num_to_str(q.1)));\n\
             0\n\
         }\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "main.nz"]),
        "nazm run",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.starts_with("abc3\n"), "{r:?}");
    let exe = d.path().join("m");
    let b = run_bounded(
        Command::new(NAZM).current_dir(d.path()).args([
            "build",
            "main.nz",
            "-o",
            exe.to_str().unwrap(),
        ]),
        "nazm build",
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let r = run_bounded(&mut common::bounded_exe(&exe), "built");
    assert!(r.stdout.starts_with("abc3\n"), "{r:?}");
}

#[test]
fn a_tuple_is_written_and_formatted_as_the_specification_spells_it() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f.nz");
    std::fs::write(
        &p,
        "fn f(p: (Int,   Str)) -> (Str,Int) { let (a,b) = p; for(x, y) in vec_new[(Int, Int)]() { } (b,a) }\n",
    )
    .unwrap();
    let r = run_bounded(Command::new(NAZM).arg("fmt").arg(&p), "nazm fmt");
    assert_eq!(r.code, Some(0), "{r:?}");
    let got = std::fs::read_to_string(&p).unwrap();
    assert!(
        got.contains("fn f(p: (Int, Str)) -> (Str, Int) { let (a, b) = p; for (x, y) in vec_new[(Int, Int)]() {")
            && got.contains("(b, a) }"),
        "{got}"
    );
}

#[test]
fn a_plain_let_is_read_as_one_and_reported_as_one() {
    // `let x = e;` is the binding it always was, not a pattern `let`: a malformed one says what
    // a `let` binding expects, as before R1a, rather than what a pattern's does.
    let (codes, first) = refused("fn main() -> Int { let x: Int = 1; x }\n");
    assert_eq!(codes, ["N0100"], "{first}");
    assert!(first.contains("in a `let` binding"), "{first}");
}
