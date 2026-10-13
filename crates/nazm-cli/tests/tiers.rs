//! N99: the execution tiers agree — the REPL, `nazm run`, and native builds by LLVM and Cranelift.
//!
//! `docs/architecture.md` §7.100. There is no JIT (BLOCKED: §7.67, §7.100); the tiers that exist
//! are held to one answer. A REPL session defines functions and evaluates expressions; each
//! expression, with the session's definitions, is then a program run by the interpreter and built
//! natively by both backends. Every tier must give the same value, or fail with the same code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const DEFINITIONS: &[&str] = &[
    "fn sq(n: Int) -> Int { n * n }",
    "fn fact(n: Int) -> Int { if n < 2 { 1 } else { n * fact(n - 1) } }",
    "fn gcd(a: Int, b: Int) -> Int { if b == 0 { a } else { gcd(b, a % b) } }",
    "fn sum_to(n: Int) -> Int { let mut i = 0; let mut s = 0; while i <= n { s = s + i; i = i + 1; } s }",
    "fn pick(c: Bool, a: Int, b: Int) -> Int { if c { a } else { b } }",
];

const EXPRESSIONS: &[&str] = &[
    "sq(7)",
    "sq(3037000500)",
    "fact(20)",
    "fact(21)",
    "gcd(1071, 462)",
    "gcd(5, 0)",
    "sum_to(1000)",
    "1 / 0",
    "-9223372036854775808 / -1",
    "-9223372036854775808 % -1",
    "-7 / 2 + -7 % 2",
    "pick(3 < 4, 10, 20) * pick(false, 1, 2)",
    "sum_to(4) - fact(4) + sq(-3)",
];

/// A tier's answer: a value, or the code it failed with.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Value(String),
    Failed(String),
}

fn code_in(text: &str) -> Option<String> {
    text.split("error[")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .map(str::to_owned)
}

/// Every evaluation's answer from one REPL session, in order.
fn repl(dir: &Path) -> Vec<Answer> {
    let mut child = Command::new(NAZM)
        .current_dir(dir)
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = String::new();
    for d in DEFINITIONS {
        input.push_str(d);
        input.push('\n');
    }
    for e in EXPRESSIONS {
        input.push_str(e);
        input.push('\n');
    }
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut answers = Vec::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("= ") {
            answers.push(Answer::Value(v.to_owned()));
        } else if let Some(c) = line.strip_prefix("error[") {
            answers.push(Answer::Failed(c.split(']').next().unwrap().to_owned()));
        }
    }
    answers
}

fn program(expr: &str) -> String {
    format!(
        "{}\nfn main() -> Int {{ {expr} }}\n",
        DEFINITIONS.join("\n")
    )
}

fn interpreted(dir: &Path, expr: &str) -> Answer {
    std::fs::write(dir.join("p.nz"), program(expr)).unwrap();
    let o = Command::new(NAZM)
        .current_dir(dir)
        .args(["run", "p.nz"])
        .output()
        .unwrap();
    if o.status.success() {
        let out = String::from_utf8(o.stdout).unwrap();
        Answer::Value(out.lines().last().unwrap_or("").to_owned())
    } else {
        Answer::Failed(code_in(&String::from_utf8_lossy(&o.stderr)).unwrap_or_default())
    }
}

fn native(dir: &Path, expr: &str, backend: &str) -> Answer {
    std::fs::write(dir.join("p.nz"), program(expr)).unwrap();
    let b = Command::new(NAZM)
        .current_dir(dir)
        .args(["build", "--backend", backend, "p.nz", "-o", "p"])
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let o = common::bounded_exe(&dir.join("p")).output().unwrap();
    if o.status.success() {
        Answer::Value(String::from_utf8(o.stdout).unwrap().trim().to_owned())
    } else {
        assert_eq!(o.status.code(), Some(2), "{expr} under {backend}");
        Answer::Failed(code_in(&String::from_utf8_lossy(&o.stderr)).unwrap_or_default())
    }
}

#[test]
fn every_tier_gives_one_answer() {
    let d = tempfile::tempdir().unwrap();
    let session = repl(d.path());
    assert_eq!(session.len(), EXPRESSIONS.len(), "{session:?}");
    let mut failures = 0;
    for (expr, from_repl) in EXPRESSIONS.iter().zip(&session) {
        let tiers = [
            ("run", interpreted(d.path(), expr)),
            ("llvm", native(d.path(), expr, "llvm")),
            ("cranelift", native(d.path(), expr, "cranelift")),
        ];
        for (tier, answer) in &tiers {
            assert_eq!(
                answer, from_repl,
                "`{expr}`: the REPL and `{tier}` disagree"
            );
        }
        failures += usize::from(matches!(from_repl, Answer::Failed(_)));
    }
    // Both kinds of answer are compared, not only values.
    assert_eq!(failures, 4, "{session:?}");
}
