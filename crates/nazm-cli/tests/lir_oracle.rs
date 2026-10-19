//! N105: LIR's interpreter is the oracle both backends are held to.
//!
//! `docs/internal/architecture.md` §7.106. Since N105 every decision a program can observe — what a value
//! costs, where a reference is taken or given back, how an operation fails — is made once, in the
//! instruction-level LIR, and the LLVM and Cranelift backends only translate it. `nazm lir --run`
//! runs that LIR directly. Here every program runs on the oracle, on LLVM at `-O0` and `-O2` and on
//! Cranelift, and all four must print the same thing, end with the same status, and fail — when a
//! program fails — with the same diagnostic at the same place. The programs cover each part of the
//! LIR a backend translates: arithmetic and its failures, strings and sequences with their
//! reference counts, records and enums in memory, equality helpers, closures and indirect calls,
//! recursion, and the program's arguments. The expected answers are worked out by hand, not
//! taken from any tier (`print` writes no newline; `main`'s result is printed after it, with one). Programs outside the oracle's subset (tasks, the clock,
//! C) are refused with status 4, and a refusal is never mistaken for an answer.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// `(stdout, exit status, the failure line)` of one tier.
type Answer = (String, Option<i32>, String);

fn failure(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .find(|l| l.contains("error[N04"))
        .map(|l| l.trim_start_matches("nazm: ").to_owned())
        .unwrap_or_default()
}

fn oracle(dir: &Path, args: &[&str]) -> (Answer, String) {
    let o = Command::new(NAZM)
        .current_dir(dir)
        .args(["lir", "--run", "prog.nz", "--"])
        .args(args)
        .output()
        .unwrap();
    (
        (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.code(),
            failure(&o.stderr),
        ),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn native(dir: &Path, backend: &str, opt: &str, args: &[&str]) -> Answer {
    let exe = dir.join(crate::common::exe_name(format!("p-{backend}-{opt}")));
    let b = Command::new(NAZM)
        .current_dir(dir)
        .args([
            "build",
            "--backend",
            backend,
            "--opt-level",
            opt,
            "prog.nz",
            "-o",
        ])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        b.status.success(),
        "{backend} -O{opt} does not build it:\n{}",
        String::from_utf8_lossy(&b.stderr)
    );
    let o = Command::new(&exe).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.code(),
        failure(&o.stderr),
    )
}

/// Every tier gives `src` the same answer, and it is `expected`'s: its output, then `main`'s
/// result, or status 2 and a failure line containing `expected` when it starts with `error[`.
fn agree(src: &str, args: &[&str], expected: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let (said, stderr) = oracle(d.path(), args);
    assert_ne!(said.1, Some(4), "the oracle refused it: {stderr}\n{src}");
    if expected.starts_with("error[") {
        assert_eq!(said.1, Some(2), "{said:?}\n{src}");
        assert!(said.2.contains(expected), "{said:?}\n{src}");
    } else {
        assert_eq!(said.1, Some(0), "{said:?} {stderr}\n{src}");
        assert_eq!(said.0, expected, "{src}");
    }
    for (backend, opt) in [("llvm", "0"), ("llvm", "2"), ("cranelift", "2")] {
        assert_eq!(
            native(d.path(), backend, opt, args),
            said,
            "{backend} -O{opt} and the LIR oracle disagree:\n{src}"
        );
    }
}

#[test]
fn integers_and_their_failures_agree_with_the_oracle() {
    agree(
        "fn f(a: Int, b: Int) -> Int { a * 3 - b / 2 + a % 7 }\n\
         fn main() -> Int { f(10, 4) + f(0 - 9, 5) + (0 - 7) % 3 + (0 - 9223372036854775807 - 1) % (0 - 1) }\n",
        &[],
        "-1\n",
    );
    // Each remainder alone, so no two wrong answers can cancel (N108 found the sum above does on
    // AArch64 Linux when a select's arms are swapped): 10 % 7 = 3, -9 % 7 = -2, 7 % -2 = 1.
    agree(
        "fn r(a: Int, b: Int) -> Int { a % b }\n\
         fn main() -> Int { r(10, 7) * 100 + r(0 - 9, 7) * 10 + r(7, 0 - 2) }\n",
        &[],
        "281\n",
    );
    agree(
        "fn main() -> Int { let big = 9223372036854775807; big + 1 }\n",
        &[],
        "error[N0400]",
    );
    agree(
        "fn d(x: Int) -> Int { 10 / x }\nfn main() -> Int { d(0) }\n",
        &[],
        "error[N0401]",
    );
}

#[test]
fn control_flow_and_recursion_agree_with_the_oracle() {
    // Each prints its answer and returns 0 (R1, 5b.3), which the result report adds.
    agree(
        include_str!("../../../examples/primes.nz"),
        &[],
        "1060\n0\n",
    );
    agree(include_str!("../../../examples/sum.nz"), &[], "6074\n0\n");
    agree(
        "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
         fn main() -> Int { let mut i = 0; let mut t = 0; while i < 15 { if i == 3 { i = i + 1; continue; } t = t + fib(i); i = i + 1; } t }\n",
        &[],
        "984\n",
    );
}

#[test]
fn strings_and_sequences_agree_with_the_oracle() {
    agree(
        "fn main(out: OutCap) -> Int {\n\
         let xs = ints_new();\n\
         let ss = strs_new();\n\
         let mut i = 0;\n\
         while i < 50 { ints_push(xs, i * i); strs_push(ss, str_concat(\"n\", int_to_str(i))); i = i + 1; }\n\
         ints_set(xs, 3, 1000);\n\
         strs_set(ss, 4, \"four\");\n\
         let joined = str_concat(strs_get(ss, 4), strs_get(ss, 49));\n\
         print(joined);\n\
         print(int_to_str(ints_get(xs, 3) + ints_get(xs, 49) + ints_len(xs) + strs_len(ss)));\n\
         print(str_slice(\"abcdef\", 1, 4));\n\
         if joined == \"fourn49\" { 0 } else { 1 }\n\
         }\n",
        &[],
        "fourn493501bcd0\n",
    );
    agree(
        "fn main() -> Int { let xs = ints_new(); ints_push(xs, 1); ints_get(xs, 5) }\n",
        &[],
        "error[N04",
    );
}

#[test]
fn records_enums_and_equality_agree_with_the_oracle() {
    agree(
        "struct Q { flag: Bool, name: Str, n: Int }\n\
         enum S { Num(n: Int), Pair(q: Q, k: Int), Empty }\n\
         fn val(s: S) -> Int { match s { S.Num(n: n) => n, S.Pair(q: q, k: k) => (if q.flag { str_len(q.name) } else { 0 }) + q.n + k, S.Empty() => 0, } }\n\
         fn main(out: OutCap) -> Int {\n\
         let a = Q(flag: true, name: str_concat(\"ab\", \"c\"), n: 10);\n\
         let b = Q(flag: true, name: \"abc\", n: 10);\n\
         let c = Q(flag: false, name: \"abc\", n: 10);\n\
         print(int_to_str(val(S.Pair(q: a, k: 4)) + val(S.Num(n: 5)) + val(S.Empty())));\n\
         print(if b == Q(flag: true, name: \"abc\", n: 10) { \"same\" } else { \"differ\" });\n\
         print(if b != c { \"differ\" } else { \"same\" });\n\
         print(if S.Num(n: 1) == S.Num(n: 1) && S.Empty() == S.Empty() && S.Num(n: 1) != S.Empty() { \"enums\" } else { \"wrong\" });\n\
         0\n\
         }\n",
        &[],
        "22samedifferenums0\n",
    );
}

#[test]
fn closures_and_vectors_agree_with_the_oracle() {
    agree(
        "struct Op { name: Str, apply: fn(Int) -> Int }\n\
         fn compose(f: fn(Int) -> Int, g: fn(Int) -> Int) -> fn(Int) -> Int { fn(x: Int) -> Int { f(g(x)) } }\n\
         fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n\
         fn fact(n: Int) -> Int { if n <= 1 { 1 } else { n * apply(fact, n - 1) } }\n\
         fn main(out: OutCap) -> Int {\n\
         let tag = str_concat(\"ab\", \"cd\");\n\
         let xs = ints_new();\n\
         ints_push(xs, 3);\n\
         let len_plus = fn(x: Int) -> Int { x + str_len(tag) + ints_get(xs, 0) };\n\
         let op = Op(name: tag, apply: len_plus);\n\
         let fs = vec_new[fn(Int) -> Int]();\n\
         vec_push(fs, len_plus);\n\
         vec_push(fs, compose(len_plus, op.apply));\n\
         let both = vec_get(fs, 1);\n\
         let ap = op.apply;\n\
         print(int_to_str(both(1) + ap(0) + fact(5)));\n\
         0\n\
         }\n",
        &[],
        "1420\n",
    );
}

#[test]
fn the_programs_arguments_agree_with_the_oracle() {
    agree(
        "use \"@std/process\";\n\
         fn main(io: IoCap, out: OutCap) -> Int {\n\
         let args = process_args(io);\n\
         print(str_join(args, \"+\"));\n\
         strs_len(args)\n\
         }\n",
        &["one", "two words"],
        "one+two words2\n",
    );
}

#[test]
fn a_program_with_tasks_is_refused_and_not_answered() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        include_str!("../../../examples/worker-pool.nz"),
    )
    .unwrap();
    let (said, stderr) = oracle(d.path(), &[]);
    assert_eq!(said.1, Some(1), "{said:?} {stderr}");
    assert!(
        stderr.contains("the LIR interpreter does not run this"),
        "{stderr}"
    );
    assert!(said.0.is_empty(), "a refusal printed an answer: {said:?}");
}

#[test]
fn a_remainder_never_divides_by_minus_one() {
    // `Int::MIN % -1` is 0 (`docs/user/spec.md`), and the machine faults on that one pair on some
    // targets and not on others (AArch64 does not), so no program's output on this host can say
    // whether the lowering kept the guard. The LIR can: the divisor is a select between 1 and the
    // operand, never the operand twice.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn r(a: Int, b: Int) -> Int { a % b }\nfn main() -> Int { r(7, 2) }\n",
    )
    .unwrap();
    let o = Command::new(NAZM)
        .current_dir(d.path())
        .args(["lir", "--ops", "prog.nz"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let body = text
        .split("function ")
        .find(|f| f.contains("; r\n"))
        .expect("r");
    let one = body
        .lines()
        .find_map(|l| l.trim().strip_suffix(" = const i64 1"))
        .expect("the constant 1");
    let select = body
        .lines()
        .find(|l| l.contains(" = select "))
        .expect("a select for the divisor");
    let arms: Vec<&str> = select
        .split(" = select ")
        .nth(1)
        .unwrap()
        .split(", ")
        .skip(1)
        .collect();
    assert_eq!(arms.len(), 2, "{select}");
    assert_ne!(
        arms[0], arms[1],
        "the divisor's select chooses nothing: {select}"
    );
    assert!(
        arms.contains(&one),
        "the divisor is never 1: {select}\n{body}"
    );
    assert!(body.contains("srem"), "{body}");
}

/// Q1-B-07: the oracle formats floats and `UInt64`s — every shape the shortest round-trip text
/// takes, `Float32` and `Float64`, the edges (`-0`, an infinity, a NaN, `1e+21`, `1.5e-7`) and the
/// largest `UInt64` — by the function the interpreter uses, so a program that prints a number now
/// has its fifth tier, held to LLVM at both levels and Cranelift.
#[test]
fn floats_and_unsigned_words_print_alike_on_the_oracle() {
    agree(
        "fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \" \")) }\n\
         fn main(out: OutCap) -> Int ! {io} {\n\
             show(out, num_to_str(1.5));\n\
             show(out, num_to_str(0.1 + 0.2));\n\
             show(out, num_to_str(1e21));\n\
             show(out, num_to_str(0.00000015));\n\
             show(out, num_to_str(100.0));\n\
             show(out, num_to_str(0.0 * (0.0 - 1.0)));\n\
             show(out, num_to_str(Float32(0.1)));\n\
             show(out, num_to_str(Float32(16777217.0)));\n\
             show(out, num_to_str(1.0 / 0.0));\n\
             show(out, num_to_str(0.0 - 1.0 / 0.0));\n\
             show(out, num_to_str(0.0 / 0.0));\n\
             show(out, num_to_str(UInt64(18446744073709551615)));\n\
             show(out, num_to_str(UInt64(0)));\n\
             0\n\
         }\n",
        &[],
        "1.5 0.30000000000000004 1e+21 1.5e-7 100 -0 0.1 16777216 inf -inf nan 18446744073709551615 0 0\n",
    );
}
