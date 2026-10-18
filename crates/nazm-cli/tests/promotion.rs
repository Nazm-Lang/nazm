//! N59: scalar temporaries as SSA values in the LLVM emitter.
//!
//! `docs/architecture.md` §7.61. What a whole build shows: a temporary of type `Int` or `Bool`
//! that MIR assigns once and reads only later in its block has no stack slot, every other local
//! keeps one, and the program means what it meant — at `-O0`, where clang promotes nothing, and at
//! `-O2`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

const PROGRAM: &str = "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
                       fn twice(x: Int) -> Int { let y = x * 2; y + 1 }\n\
                       fn main() -> Int {\n    let mut i = 0;\n    let mut s = 0;\n    \
                       while i < 2000 { s = s + i % 7 * 3; i = i + 1; }\n    \
                       let ok = s > 0 && i == 2000;\n    \
                       let text = str_concat(\"s=\", int_to_str(s));\n    \
                       if ok { str_len(text) + fib(15) + twice(0) - 1 } else { 0 }\n}\n";

/// The slots each function's text declares, by the comment the emitter writes beside each.
fn slots(ir: &str) -> Vec<String> {
    ir.lines()
        .filter(|l| l.contains(" = alloca "))
        .filter_map(|l| l.split("; ").nth(1).map(|c| c.trim().to_owned()))
        .collect()
}

#[test]
fn a_scalar_temporary_has_no_slot_and_every_other_local_keeps_one() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let b = nazm(
        d.path(),
        &[
            "build",
            "--no-cache",
            "--emit-ir",
            "--opt-level",
            "0",
            "prog.nz",
            "-o",
            "prog",
        ],
    );
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let ir = std::fs::read_to_string(d.path().join("prog.m0.ll")).unwrap();
    let have = slots(&ir);
    // Parameters, bindings, managed values and a temporary assigned in two blocks (an `if`'s
    // result) keep their slots; every scalar temporary assigned once and read in its block —
    // eleven `arg#` here — has none.
    assert_eq!(
        have,
        [
            "n : Int",
            "result#1 : Int",
            // A binding assigned once and read in its block keeps its slot: a debugger reads it.
            "x : Int",
            "y : Int",
            // `main`'s parameter, the `out: OutCap` it is given (R1, 5b.1); a capability is a
            // word, and every local numbered after it moves up by one.
            "out : Int",
            "i : Int",
            "s : Int",
            "ok : Bool",
            "result#5 : Int",
            // Then the values held in memory (N105: LIR's slots follow its variables).
            "text : Str",
            "arg#10 : Str",
        ],
        "{ir}"
    );
    // The program means what it meant, at both optimisation levels.
    for opt in ["0", "2"] {
        let exe = d.path().join(crate::common::exe_name(format!("o{opt}")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert!(b.status.success());
        let r = Command::new(&exe).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&r.stdout), "617\n", "-O{opt}");
    }
    let run = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "617\n");
}
