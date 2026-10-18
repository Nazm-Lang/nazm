//! R1a: patterns — wildcards, alternatives, nesting and literals, `if let` and `let … else`
//! (`docs/spec.md`, *Patterns*).
//!
//! Every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift
//! — is asked for what the section's rules work out by hand, with every allocation reclaimed;
//! then what exhaustiveness and reachability refuse, by which code; and that a `match` written
//! variant by variant still says what it said before R1a.

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
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
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
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
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

const PATTERNS: &str = "enum Expr { Num(v: Int), Add(l: Box[Expr], r: Box[Expr]), Neg(e: Box[Expr]), }
enum Shape { Circle(r: Int), Rect(w: Int, h: Int), Dot, }
enum Wrap { Item(label: Str, tags: Strs), Empty, }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }

// Literal and binding patterns nested in a variant; or-patterns without bindings.
fn describe(e: Expr) -> Str {
    match e {
        Expr.Num(v: 0) => \"zero\",
        Expr.Num(v: n) => num_to_str(n),
        Expr.Add(l: _, r: _) | Expr.Neg(e: _) => \"compound\",
    }
}

// Or-patterns that bind, arms in order, and `_` at the end.
fn area(s: Shape) -> Int {
    match s {
        Shape.Rect(w: 0, h: _) | Shape.Rect(w: _, h: 0) => 0,
        Shape.Rect(w: w, h: h) => w * h,
        Shape.Circle(r: r) | Shape.Rect(w: r, h: 1) => r,
        _ => 1,
    }
}

// Integers, negative ones, alternatives, and a name over the rest.
fn word(n: Int) -> Str {
    match n { 0 => \"none\", 1 | 2 => \"few\", -1 => \"minus one\", other => num_to_str(other), }
}

fn code(s: Str) -> Int { match s { \"a\" => 1, \"bc\" => 2, _ => 0, } }
fn flag(b: Bool) -> Int { match b { true => 1, false => 0, } }

// `let … else`: the bindings are in scope for the rest of the block.
fn or_zero(o: Option[Int]) -> Int {
    let Option.Some(value: x) = o else { return 0; };
    x + 100
}

// A string literal and a name binding an owning payload, nested; the scrutinee and the
// payloads released whichever arm runs.
fn tag_count(w: Wrap) -> Int {
    match w {
        Wrap.Item(label: \"skip\", tags: _) => 0,
        Wrap.Item(label: _, tags: tags) => strs_len(tags),
        Wrap.Empty() => -1,
    }
}

fn main(out: OutCap) -> Int ! {io} {
    show(out, describe(Expr.Num(v: 0)));
    show(out, describe(Expr.Num(v: 7)));
    show(out, describe(Expr.Neg(e: box_new(Expr.Num(v: 1)))));
    show(out, num_to_str(area(Shape.Rect(w: 0, h: 9)) + area(Shape.Rect(w: 3, h: 4)) * 10
        + area(Shape.Circle(r: 5)) * 1000 + area(Shape.Dot()) * 100000
        + area(Shape.Rect(w: 6, h: 1)) * 1000000));
    show(out, str_concat(word(0), str_concat(word(2), str_concat(word(-1), word(9)))));
    show(out, num_to_str(code(\"a\") + code(\"bc\") * 10 + code(\"zz\") * 100 + flag(true) * 1000));
    show(out, num_to_str(or_zero(Option[Int].Some(value: 5)) + or_zero(Option[Int].None())));
    if let Expr.Num(v: k) = Expr.Num(v: 42) { show(out, num_to_str(k)); } else { show(out, \"no\"); }
    if let Shape.Dot() = Shape.Circle(r: 1) { show(out, \"dot\"); }
    let ts = strs_new();
    strs_push(ts, \"x\");
    strs_push(ts, \"y\");
    let total = tag_count(Wrap.Item(label: \"keep\", tags: ts))
        + tag_count(Wrap.Item(label: \"skip\", tags: ts)) * 10
        + tag_count(Wrap.Empty()) * 100;
    show(out, num_to_str(total));
    0
}
";

/// What the section's rules give, worked out by hand: `area` is 0 + 12·10 + 5·1000 + 1·100000 +
/// 6·1000000 (a `Rect` of height 1 is the third arm's, binding `r` to the width).
const EXPECTED: &str = "zero\n7\ncompound\n6105120\nnonefewminus one9\n1021\n105\n42\n-98\n";

#[test]
fn every_kind_of_pattern_means_the_same_everywhere_and_is_reclaimed() {
    for (label, r) in everywhere(PATTERNS) {
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

const HEADER: &str = "enum S { A, B(x: Int), C(y: Bool), }\n";

#[test]
fn exhaustiveness_names_a_value_no_arm_matches() {
    for (body, code, says) in [
        (
            "fn f(s: S) -> Int { match s { S.A() => 1, S.B(x: 0) => 2, S.C(y: _) => 3, } }",
            "N0343",
            "S.B(x: _)",
        ),
        (
            "fn f(s: S) -> Int { match s { S.A() => 1, S.B(x: _) => 2, S.C(y: true) => 3, } }",
            "N0343",
            "S.C(y: false)",
        ),
        (
            "fn f(n: Int) -> Int { match n { 0 => 1, 1 => 2, } }",
            "N0343",
            "`_`",
        ),
        // Written variant by variant, the message is what it was before R1a.
        (
            "fn f(s: S) -> Int { match s { S.A() => 1, } }",
            "N0343",
            "`B`, `C`",
        ),
    ] {
        let (codes, first) = refused(&format!("{HEADER}{body}\nfn main() -> Int {{ 0 }}\n"));
        assert_eq!(codes.first().map(String::as_str), Some(code), "{body}");
        assert!(first.contains(says), "{body}: {first}");
    }
}

#[test]
fn what_a_pattern_refuses_and_by_which_code() {
    for (body, code) in [
        // Unreachable: every value an earlier arm matches.
        (
            "fn f(n: Int) -> Int { match n { _ => 1, 0 => 2, } }",
            "N0344",
        ),
        (
            "fn f(s: S) -> Int { match s { S.A() => 1, S.A() => 2, _ => 3, } }",
            "N0344",
        ),
        // An `if let` that cannot fail leaves its `else` unreachable.
        (
            "fn f(n: Int) -> Int { if let k = n { k } else { 0 } }",
            "N0344",
        ),
        // An or-pattern's alternatives bind the same names, of the same types.
        (
            "fn f(s: S) -> Int { match s { S.B(x: v) | S.A() => 1, _ => 2, } }",
            "N0629",
        ),
        (
            "fn f(s: S) -> Int { match s { S.B(x: v) | S.C(y: v) => 1, _ => 2, } }",
            "N0629",
        ),
        // A `let … else`'s `else` must leave.
        (
            "fn f(o: Option[Int]) -> Int { let Option.Some(value: x) = o else { 0; }; x }",
            "N0630",
        ),
        // No float literal; a literal of the value's type; a variant only against an enum.
        (
            "fn f(x: Float64) -> Int { match x { 1.5 => 1, _ => 2, } }",
            "N0300",
        ),
        (
            "fn f(n: Int) -> Int { match n { \"a\" => 1, _ => 2, } }",
            "N0300",
        ),
        (
            "fn f(n: Int) -> Int { match n { S.A() => 1, _ => 2, } }",
            "N0342",
        ),
        // An empty `match` of something not an enum, as before R1a.
        ("fn f(n: Int) -> Int { match n { } }", "N0342"),
    ] {
        let (codes, _) = refused(&format!("{HEADER}{body}\nfn main() -> Int {{ 0 }}\n"));
        assert_eq!(codes.first().map(String::as_str), Some(code), "{body}");
    }
}

#[test]
fn a_wrong_pattern_is_one_error_not_a_cascade() {
    // The arm whose pattern is the mistake is not checked further, so its bindings are not
    // reported again as unknown names.
    let (codes, _) = refused(
        "enum A { X(v: Int), }\nenum B { X(v: Int), }\n\
         fn main() -> Int { match A.X(v: 1) { B.X(v: n) => n, } }\n",
    );
    assert_eq!(codes, ["N0345"]);
}
