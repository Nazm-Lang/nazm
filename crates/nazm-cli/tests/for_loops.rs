//! R1a: `for` loops (`docs/spec.md`, *`for` loops*).
//!
//! Each form means the `while` loop the specification writes beside it, so the tests ask two
//! things of every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and
//! Cranelift: that a program of `for` loops prints what the specification's rules work out by
//! hand, and that it prints and reclaims exactly what the same program written with those
//! `while` loops does. Then what is refused, by which code, and that `in` is still a name.

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

/// The program under the interpreter and the LIR oracle, and built and run by each backend,
/// each with the memory report asked for.
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

/// The memory report's line, which every implementation but the LIR oracle writes when asked:
/// the oracle executes LIR for its output and keeps no allocation counts.
fn report(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"))
        .to_owned()
}

fn nothing_live(line: &str) -> bool {
    line.split_whitespace()
        .filter(|w| w.starts_with("live="))
        .all(|w| w == "live=0")
}

const HEADER: &str = "struct P { name: Str, n: Int, }

fn show(out: OutCap, s: Str) -> Int ! {io} {
    print(str_concat(s, \"\\n\"))
}
";

/// Every rule of the section, each loop printing one line.
const LOOPS: &str = "
fn main(out: OutCap) -> Int ! {io} {
    // An element pushed during the loop is visited: the length is read before every step.
    let xs = ints_new();
    ints_push(xs, 1);
    ints_push(xs, 2);
    ints_push(xs, 3);
    let mut a = \"\";
    for x in xs {
        a = str_concat(a, num_to_str(x));
        if x == 1 { ints_push(xs, 9); }
    }
    show(out, a);
    // A pop that shortens the sequence ends the loop sooner.
    let mut b = \"\";
    for x in xs {
        b = str_concat(b, num_to_str(x));
        b = str_concat(b, num_to_str(ints_pop(xs)));
    }
    show(out, b);
    // `continue` moves on, `break` leaves.
    let ss = strs_new();
    strs_push(ss, \"a\");
    strs_push(ss, \"skip\");
    strs_push(ss, \"b\");
    strs_push(ss, \"stop\");
    strs_push(ss, \"c\");
    let mut c = \"\";
    for s in ss {
        if s == \"skip\" { continue; }
        if s == \"stop\" { break; }
        c = str_concat(c, s);
    }
    show(out, c);
    // Records, with a range nested inside, bounded by a field.
    let ps = vec_new[P]();
    vec_push(ps, P(name: \"x\", n: 2));
    vec_push(ps, P(name: \"y\", n: 0));
    vec_push(ps, P(name: \"z\", n: 1));
    let mut d = \"\";
    for p in ps {
        for k in 0..p.n { d = str_concat(d, p.name); }
        d = str_concat(d, \".\");
    }
    show(out, d);
    // Bytes, each a `UInt8`.
    let bs = bytes_from_str(\"AB\");
    let mut e = 0;
    for v in bs { e = e + Int(v); }
    show(out, num_to_str(e));
    // Ranges: an empty and a backwards one run no step; a negative start counts up.
    let mut f = \"\";
    for i in 3..3 { f = str_concat(f, \"never\"); }
    for i in 5..2 { f = str_concat(f, \"never\"); }
    for i in -2..1 { f = str_concat(f, num_to_str(i)); }
    show(out, f);
    // `in` is still an ordinary name.
    let in = 7;
    show(out, num_to_str(in));
    0
}
";

/// What the section's rules give, worked out by hand.
const EXPECTED: &str = "1239\n1923\nab\nxx..z.\n131\n-2-10\n7\n";

#[test]
fn every_rule_holds_in_every_implementation_and_everything_is_reclaimed() {
    let src = format!("{HEADER}{LOOPS}");
    for (label, r) in everywhere(&src) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {}", r.stdout);
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

/// The first two loops above, written as the `while` loops the specification says they mean —
/// with the hidden names spelled out — and the same loops written with `for`.
const AS_FOR: &str = "
fn main(out: OutCap) -> Int ! {io} {
    let xs = ints_new();
    ints_push(xs, 1);
    ints_push(xs, 2);
    let ps = vec_new[P]();
    vec_push(ps, P(name: \"x\", n: 2));
    vec_push(ps, P(name: \"y\", n: 1));
    let mut a = \"\";
    for x in xs {
        if x == 1 { ints_push(xs, 9); continue; }
        a = str_concat(a, num_to_str(x));
    }
    for p in ps {
        if p.n == 1 { break; }
        for k in 0..p.n { a = str_concat(a, p.name); }
    }
    show(out, a);
    0
}
";

const AS_WHILE: &str = "
fn main(out: OutCap) -> Int ! {io} {
    let xs = ints_new();
    ints_push(xs, 1);
    ints_push(xs, 2);
    let ps = vec_new[P]();
    vec_push(ps, P(name: \"x\", n: 2));
    vec_push(ps, P(name: \"y\", n: 1));
    let mut a = \"\";
    {
        let seq = xs;
        let mut i = 0;
        while i < ints_len(seq) {
            let x = ints_get(seq, i);
            i = i + 1;
            if x == 1 { ints_push(xs, 9); continue; }
            a = str_concat(a, num_to_str(x));
        }
    }
    {
        let seq = ps;
        let mut i = 0;
        while i < vec_len(seq) {
            let p = vec_get(seq, i);
            i = i + 1;
            if p.n == 1 { break; }
            {
                let mut j = 0;
                let end = p.n;
                while j < end {
                    let k = j;
                    j = j + 1;
                    a = str_concat(a, p.name);
                }
            }
        }
    }
    show(out, a);
    0
}
";

#[test]
fn a_for_loop_prints_and_reclaims_exactly_what_its_while_loop_does() {
    let with_for = everywhere(&format!("{HEADER}{AS_FOR}"));
    let with_while = everywhere(&format!("{HEADER}{AS_WHILE}"));
    for ((label, f), (_, w)) in with_for.iter().zip(&with_while) {
        assert_eq!(f.code, Some(0), "{label}: {f:?}");
        assert_eq!(f.stdout, w.stdout, "{label}");
        assert!(f.stdout.starts_with("29xx\n"), "{label}: {}", f.stdout);
        if label != "lir oracle" {
            assert_eq!(report(f), report(w), "{label}");
        }
    }
}

/// `nazm check` on `src`: its exit status and its first error's code.
fn first_error(src: &str) -> (Option<i32>, Option<String>) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("r.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "r.nz"]),
        "nazm check",
    );
    let code = r.stderr.split("error[").nth(1).map(|s| s[..5].to_owned());
    (r.code, code)
}

#[test]
fn what_a_for_loop_refuses_and_by_which_code() {
    for (src, code) in [
        // Not a sequence (`N0628`).
        ("fn main() -> Int { for x in 5 { } 0 }", "N0628"),
        // A `for` of neither form is still refused by name, as before R1a.
        ("fn main() -> Int { for x { 1 } }", "N0101"),
        (
            "fn main() -> Int { let xs = ints_new(); for for in xs { } 0 }",
            "N0101",
        ),
        // The name is immutable, new each step, and the body's.
        (
            "fn main() -> Int { let xs = ints_new(); for x in xs { x = 1; } 0 }",
            "N0310",
        ),
        (
            "fn main() -> Int { let xs = ints_new(); for x in xs { let x = 1; } 0 }",
            "N0202",
        ),
        (
            "fn main() -> Int { let xs = ints_new(); for x in xs { } x }",
            "N0200",
        ),
        // A range's bounds are `Int`s. (A loop body's last value was refused here until R1,
        // 5b.19, which discards it.)
        ("fn main() -> Int { for i in 0..\"a\" { } 0 }", "N0300"),
        // `a..b` is written only in a `for` loop.
        ("fn main() -> Int { let r = 0..3; 0 }", "N0100"),
        // `break` outside a loop, after one.
        ("fn main() -> Int { for i in 0..3 { } break; 0 }", "N0312"),
    ] {
        let (status, first) = first_error(src);
        assert_eq!(status, Some(1), "{src}");
        assert_eq!(first.as_deref(), Some(code), "{src}");
    }
}

#[test]
fn a_name_outside_shadows_nothing_and_in_is_a_name_everywhere_else() {
    let src = "fn in(in: Int) -> Int { in + 1 }
fn main() -> Int {
    let x = 1;
    let xs = ints_new();
    ints_push(xs, 5);
    let mut t = 0;
    for x in xs { t = t + x; }
    let y = in(x);
    t + y
}
";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "--interpret", "p.nz"]),
        "nazm run",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    // The loop's `x` is the body's: after it, `x` is the outer `1` again.
    assert!(r.stdout.starts_with("7"), "{}", r.stdout);
}

#[test]
fn the_formatter_keeps_a_range_glued_and_is_idempotent() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("f.nz");
    std::fs::write(
        &path,
        "fn main() -> Int {\n    let mut t = 0;\n    for i in 0 .. 3 {\n        t = t + i;\n    }\n    t\n}\n",
    )
    .unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["fmt", "f.nz"]),
        "nazm fmt",
    );
    assert_eq!(r.code, Some(0), "{r:?}");
    let once = std::fs::read_to_string(&path).unwrap();
    assert!(once.contains("for i in 0..3 {"), "{once}");
    let again = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["fmt", "--check", "f.nz"]),
        "nazm fmt --check",
    );
    assert_eq!(again.code, Some(0), "{again:?}");
}

/// Every error code `nazm check` reports for `src`, in order.
fn codes(src: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    r.stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect()
}

#[test]
fn an_element_carries_its_sequences_provenance_and_a_range_its_starts() {
    // A file's contents, through a sequence and a `for` loop, may not name a written file
    // (`N0372`), as through `strs_get` they may not; a literal through the same loop may.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); \
             strs_push(xs, read_file(\"cfg\")); \
             for p in xs { write_file(p, \"x\"); } 0 }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); \
             strs_push(xs, \"out.txt\"); \
             for p in xs { write_file(p, \"x\"); } 0 }\n"
        ),
        Vec::<String>::new()
    );
    // The cell is the sequence's type's: since R1 (5b.18) a `Strs` is a `Vec[Str]`, one type
    // and one cell, so a file's contents in one reach the elements of every other.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); \
             strs_push(xs, read_file(\"cfg\")); \
             let ys = vec_new[Str](); vec_push(ys, \"out.txt\"); \
             for p in ys { write_file(p, \"x\"); } 0 }\n"
        ),
        ["N0372"]
    );
    // A `Vec[Int]` is another type, and another cell.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let xs = strs_new(); \
             strs_push(xs, read_file(\"cfg\")); \
             let ys = vec_new[Int](); vec_push(ys, 1); \
             for n in ys { write_file(num_to_str(n), \"x\"); } 0 }\n"
        ),
        Vec::<String>::new()
    );
    // A range's name is its start's: a file's length as the start restricts it; as the end,
    // only compared, it does not.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { \
             for i in str_len(read_file(\"cfg\"))..9 { write_file(num_to_str(i), \"x\"); } 0 }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { \
             for i in 0..str_len(read_file(\"cfg\")) { write_file(num_to_str(i), \"x\"); } 0 }\n"
        ),
        Vec::<String>::new()
    );
}
