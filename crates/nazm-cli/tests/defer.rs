//! R1b, L12: `defer` (`docs/spec.md`, *Scope-exit cleanup: `defer`*).
//!
//! A deferred block runs whenever its block is left — at its end, or by `return`, `?`, `break`
//! or `continue` — the last registered first, after a returned value is computed. The tests ask
//! every implementation for what the rules work out by hand with nothing live, refuse a body that
//! would leave, keep `defer` a name elsewhere, and follow provenance to where the body runs.

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

/// The memory report's line, which every implementation but the LIR oracle writes when asked.
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

const DEFERS: &str = "fn say(out: OutCap, s: Str) -> Int ! {io} { print(s) }

fn order(out: OutCap) -> Int ! {io} {
    defer { say(out, \"a\"); }
    defer { say(out, \"b\"); }
    { defer { say(out, \"inner\"); } say(out, \"[block]\"); }
    say(out, \"[body]\");
    0
}

fn early(out: OutCap, n: Int) -> Int ! {io} {
    defer { say(out, \"E\"); }
    if n > 1 { return n * 10; }
    defer { say(out, \"F\"); }
    n
}

fn loops(out: OutCap) -> Int ! {io} {
    let mut i = 0;
    while i < 4 {
        defer { say(out, num_to_str(i)); }
        i = i + 1;
        if i == 2 { continue; }
        if i == 3 { break; }
        say(out, \".\");
    }
    i
}

fn parse(s: Str) -> Result[Int, Str] { if str_len(s) > 0 { Result[Int, Str].Ok(value: str_len(s)) } else { Result[Int, Str].Err(error: \"empty\") } }

fn tries(out: OutCap, s: Str) -> Result[Int, Str] ! {io} {
    defer { say(out, \"T\"); }
    let n = parse(s)?;
    Result[Int, Str].Ok(value: n + 1)
}

fn main(out: OutCap) -> Int ! {io} {
    order(out); say(out, \"\\n\");
    say(out, num_to_str(early(out, 5))); say(out, \"|\");
    say(out, num_to_str(early(out, 1))); say(out, \"\\n\");
    say(out, num_to_str(loops(out))); say(out, \"\\n\");
    let a = match tries(out, \"abc\") { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, };
    let b = match tries(out, \"\") { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, };
    say(out, str_concat(num_to_str(a), num_to_str(b))); say(out, \"\\n\");
    0
}
";

#[test]
fn deferred_blocks_run_on_every_way_out_everywhere_and_nothing_is_live() {
    for (label, r) in everywhere(DEFERS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout
                .starts_with("[block]inner[body]ba\nE50|FE1\n.1233\nTT4-1\n"),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

/// `nazm check` on `src`: every error code it reports, in order.
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
fn a_deferred_body_leaves_nothing() {
    let none: Vec<&str> = Vec::new();
    for (src, want) in [
        (
            "fn main() -> Int { defer { return 1; } 0 }\n",
            vec!["N0631"],
        ),
        (
            "fn main() -> Int { let mut i = 0; while i < 3 { defer { break; } i = i + 1; } 0 }\n",
            vec!["N0631"],
        ),
        (
            "fn f() -> Result[Int, Str] { defer { let x = Result[Int, Str].Ok(value: 1)?; } \
             Result[Int, Str].Ok(value: 2) }\nfn main() -> Int { 0 }\n",
            vec!["N0631"],
        ),
        // Its last line's value is discarded, as a loop body's is (R1, 5b.19).
        ("fn main() -> Int { defer { 5 } 0 }\n", none.clone()),
        // So its branches need not agree, as a statement `if`'s need not.
        (
            "fn main() -> Int { defer { if 1 < 2 { 5 } else { \"five\" } } 0 }\n",
            none.clone(),
        ),
        // A loop inside the body may leave itself; a closure inside it may return.
        (
            "fn main() -> Int { defer { let mut j = 0; while j < 2 { j = j + 1; \
             if j == 1 { continue; } break; } } 0 }\n",
            none.clone(),
        ),
        (
            "fn main() -> Int { defer { let f = fn() -> Int { return 3; }; f(); } 0 }\n",
            none.clone(),
        ),
        // A name everywhere else, as in 1.0.
        ("fn main() -> Int { let defer = 2; defer }\n", none.clone()),
    ] {
        let want: Vec<String> = want.iter().map(|c| (*c).to_owned()).collect();
        assert_eq!(codes(src), want, "{src}");
    }
}

#[test]
fn a_deferred_body_sees_what_is_assigned_after_it() {
    // A file's contents assigned after the `defer` reach the write it makes as its block ends.
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let mut p = \"out.txt\"; \
             { defer { write_file(p, \"x\"); } p = read_file(\"cfg\"); } 0 }\n"
        ),
        ["N0372"]
    );
    assert_eq!(
        codes(
            "fn main(io: IoCap) -> Int ! { io } { let mut p = \"out.txt\"; \
             { defer { write_file(p, \"x\"); } p = \"b.txt\"; } 0 }\n"
        ),
        Vec::<String>::new()
    );
}
