//! N87: contracts and the obligation census.
//!
//! `docs/architecture.md` §7.88 and the spec section *Contracts*. What only whole programs show:
//! that a `requires` and an `ensures` mean the same on the interpreter and both native backends —
//! on every return path, a `?` included, with a `Str` result reclaimed — and fail with `N0410` and
//! `N0411` at the clause; that a call of literals is proved or refused at compile time (`N0615`);
//! that a clause is only what cannot fail for want of authority or memory (`N0614`); that
//! `nazm obligations` lists every obligation with its status and witness; that `critical` and
//! `cyber` refuse an obligation of unknown status; and that a callee's preconditions reach an
//! importer through its interface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn program(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    d
}

fn codes(o: &Output) -> Vec<String> {
    let mut c: Vec<String> = err(o)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_owned))
        .collect();
    c.sort();
    c.dedup();
    c
}

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    /// The failure's first line, without where a native build printed it from.
    failure: Option<String>,
}

/// The program run by the interpreter and built by both backends: all three must agree on what
/// was printed, how the run ended, and the failure's code and sentence.
fn everywhere(src: &str) -> Ran {
    let d = program(&[("prog.nz", src)]);
    let failure = |stderr: &str| {
        stderr.lines().find(|l| l.contains("error[N")).map(|l| {
            let at = l.find("error[N").unwrap_or(0);
            let l = &l[at..];
            l.split(" at ").next().unwrap_or(l).to_owned()
        })
    };
    let interpreted = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    // The interpreter prints a program's output and `main`'s result as a native binary does.
    let mut runs = vec![Ran {
        code: interpreted.status.code(),
        stdout: out(&interpreted),
        failure: failure(&err(&interpreted)),
    }];
    for backend in ["llvm", "cranelift"] {
        let exe = d
            .path()
            .join(crate::common::exe_name(format!("prog-{backend}")));
        let b = nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert!(b.status.success(), "{backend}: {}", err(&b));
        let o = common::bounded_exe(&exe)
            .env(nazm_lir::MEMORY_VARIABLE, "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        if let Some(line) = stderr.lines().find(|l| l.starts_with("nazm-memory:")) {
            assert!(
                line.contains("strings allocated=") && !line.contains("live=1"),
                "{backend}: {line}"
            );
        }
        runs.push(Ran {
            code: o.status.code(),
            stdout: out(&o),
            failure: failure(&stderr),
        });
    }
    assert_eq!(runs[1], runs[2], "LLVM and Cranelift disagree");
    assert_eq!(
        runs[0].code, runs[1].code,
        "the interpreter disagrees: {runs:?}"
    );
    assert_eq!(
        runs[0].failure, runs[1].failure,
        "the interpreter disagrees: {runs:?}"
    );
    runs.remove(1)
}

const ISQRT: &str = "fn isqrt(n: Int) -> Int ! {}\n\
                     \x20   requires n >= 0\n\
                     \x20   ensures result >= 0\n\
                     \x20   ensures result * result <= n && n - result * result <= 2 * result\n\
                     {\n\
                     \x20   let mut r = 0;\n\
                     \x20   while (r + 1) * (r + 1) <= n { r = r + 1; }\n\
                     \x20   r\n\
                     }\n";

#[test]
fn a_contract_holds_or_fails_alike_on_every_tier() {
    let ok = everywhere(&format!(
        "{ISQRT}fn main(io: IoCap) -> Int ! {{ io }} {{\n    \
         print(int_to_str(isqrt(0))); print(\" \"); print(int_to_str(isqrt(99))); print(\" \");\n    \
         print(int_to_str(isqrt(str_len(\"abcdefghi\")))); 0\n}}\n"
    ));
    assert_eq!(ok.code, Some(0), "{ok:?}");
    assert!(ok.stdout.starts_with("0 9 3"), "{ok:?}");
    // A precondition broken at run time: `N0410` at the clause, before the body runs.
    let pre = everywhere(&format!(
        "{ISQRT}fn main(io: IoCap) -> Int ! {{ io }} {{\n    \
         print(\"before \"); let n = 0 - str_len(\"abc\"); print(int_to_str(isqrt(n))); 0\n}}\n"
    ));
    assert_eq!(pre.code, Some(2));
    assert_eq!(pre.stdout, "before ");
    assert_eq!(
        pre.failure.as_deref(),
        Some("error[N0410]: this precondition does not hold")
    );
}

#[test]
fn a_postcondition_is_checked_on_every_return_a_question_mark_included() {
    let src = |arg: &str| {
        format!(
            "fn grow(n: Int) -> Int ! {{}}\n    ensures result > n\n{{\n    \
             if n > 10 {{ return n; }}\n    n + 1\n}}\n\
             fn parse(s: Str) -> Result[Int, Str] ! {{}}\n    \
             ensures match_ok(result)\n{{\n    \
             let n = digits(s)?;\n    Result[Int, Str].Ok(value: n)\n}}\n\
             fn match_ok(r: Result[Int, Str]) -> Bool ! {{}} {{ match r {{ Result.Ok(value: v) => true, Result.Err(error: e) => false }} }}\n\
             fn digits(s: Str) -> Result[Int, Str] ! {{}} {{ if str_len(s) > 0 {{ Result[Int, Str].Ok(value: str_len(s)) }} else {{ Result[Int, Str].Err(error: \"empty\") }} }}\n\
             fn main(io: IoCap) -> Int ! {{ io }} {{ {arg} }}\n"
        )
    };
    // The tail and the early `return` both answer to `ensures`.
    let tail = everywhere(&src("print(int_to_str(grow(3))); 0"));
    assert_eq!((tail.code, tail.stdout.as_str()), (Some(0), "40\n"));
    let early = everywhere(&src("print(int_to_str(grow(20))); 0"));
    assert_eq!(early.code, Some(2));
    assert_eq!(
        early.failure.as_deref(),
        Some("error[N0411]: this postcondition does not hold")
    );
    // A `?` that leaves with the error leaves through the postcondition too.
    let ok = everywhere(&src(
        "match parse(\"12\") { Result.Ok(value: v) => print(int_to_str(v)), Result.Err(error: e) => print(e) }; 0",
    ));
    assert_eq!((ok.code, ok.stdout.as_str()), (Some(0), "20\n"));
    let failed = everywhere(&src(
        "match parse(\"\") { Result.Ok(value: v) => print(int_to_str(v)), Result.Err(error: e) => print(e) }; 0",
    ));
    assert_eq!(failed.code, Some(2));
    assert_eq!(
        failed.failure.as_deref(),
        Some("error[N0411]: this postcondition does not hold")
    );
}

#[test]
fn a_string_result_is_held_for_its_postcondition_and_reclaimed() {
    let r = everywhere(
        "fn shout(s: Str) -> Str ! {}\n    requires str_len(s) > 0\n    \
         ensures str_len(result) == str_len(s) + 1\n{ str_concat(s, \"!\") }\n\
         fn main(io: IoCap) -> Int ! { io } {\n    \
         let mut i = 0;\n    while i < 100 { let t = shout(int_to_str(i)); i = i + 1; }\n    \
         print(shout(\"hey\")); 0\n}\n",
    );
    assert_eq!((r.code, r.stdout.as_str()), (Some(0), "hey!0\n"));
}

#[test]
fn a_clause_that_itself_fails_fails_the_call_with_its_own_failure() {
    let r = everywhere(
        "fn sq(n: Int) -> Int ! {}\n    ensures result == n * n\n{ n }\n\
         fn main() -> Int { sq(9223372036854775807) }\n",
    );
    assert_eq!(r.code, Some(2));
    assert_eq!(
        r.failure.as_deref(),
        Some("error[N0400]: this multiplication overflows `Int`")
    );
}

// --- what the checker decides ------------------------------------------------------------------

#[test]
fn a_call_of_literals_is_proved_or_refused_at_compile_time() {
    let d = program(&[(
        "prog.nz",
        &format!("{ISQRT}fn main() -> Int {{ isqrt(-4) + isqrt(16) }}\n"),
    )]);
    let o = nazm(d.path(), &["check", "--json", "prog.nz"]);
    assert_eq!(codes(&o), ["N0615"]);
    std::fs::write(
        d.path().join("prog.nz"),
        format!("{ISQRT}fn main() -> Int {{ isqrt(16) + isqrt(str_len(\"ab\")) }}\n"),
    )
    .unwrap();
    let o = nazm(d.path(), &["obligations", "--json", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let calls: Vec<(&str, Option<&str>)> = v["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["kind"] == "call-precondition")
        .map(|o| (o["status"].as_str().unwrap(), o["witness"].as_str()))
        .collect();
    assert_eq!(
        calls,
        [
            ("proved", Some("n >= 0 holds with n = 16")),
            ("checked", None)
        ]
    );
}

#[test]
fn a_clause_is_only_what_cannot_fail_for_want_of_authority_or_memory() {
    let main = "fn main() -> Int { 0 }\n";
    let cases: &[(&str, &[&str])] = &[
        // A file write: the clause's function holds no `IoCap`. (Printing would not show the
        // authority refusal: since D8 every function of the program holds `main`'s output.)
        (
            "fn f(x: Int) -> Int ! {} requires write_file(\"f\", \"x\") == 0 { x }\n",
            &["N0366", "N0369", "N0614"],
        ),
        (
            "fn g(x: Int) -> Int { x }\nfn f(x: Int) -> Int ! {} requires g(x) > 0 { x }\n",
            &["N0614"],
        ),
        (
            "fn f(x: Int) -> Int ! {} requires str_len(int_to_str(x)) > 0 { x }\n",
            &["N0614"],
        ),
        (
            "fn f(x: Int) -> Int ! {} requires if x > 0 { true } else { false } { x }\n",
            &["N0614"],
        ),
        ("fn f(x: Int) -> Int ! {} requires x { x }\n", &["N0300"]),
        (
            "fn f(x: Int) -> Int ! {} ensures x > 0 requires x > 0 { x }\n",
            &["N0100"],
        ),
        (
            "fn f(x: Int) -> Int ! {} requires result > 0 { x }\n",
            &["N0200"],
        ),
    ];
    for (src, want) in cases {
        let d = program(&[("prog.nz", &format!("{src}{main}"))]);
        let o = nazm(d.path(), &["check", "--json", "prog.nz"]);
        assert_eq!(codes(&o), *want, "{src}");
    }
    // A pure function of this module, a pure built-in that only reads, a field: each allowed.
    let fine = "struct P { x: Int }\nfn pos(n: Int) -> Bool ! {} { n > 0 }\n\
                fn f(p: P, s: Str) -> Int ! {} requires pos(p.x) && str_len(s) < 10 { p.x }\n";
    let d = program(&[("prog.nz", &format!("{fine}{main}"))]);
    let o = nazm(d.path(), &["check", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
}

// --- the census -------------------------------------------------------------------------------

#[test]
fn the_census_lists_every_obligation_with_its_status() {
    let d = program(&[(
        "prog.nz",
        "fn half(n: Int) -> Int ! {}\n    requires n >= 0\n    ensures result * 2 <= n\n{ n / 2 }\n\
         fn ap(f: fn(Int) -> Int, x: Int) -> Int ! {} { f(x) }\n\
         fn main() -> Int { half(8) + str_byte(\"a\", 0) + 2 * 3 + ap(half, 4) }\n",
    )]);
    let o = nazm(d.path(), &["obligations", "--json", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["schema"], "nazm.obligations/1");
    assert_eq!(v["evidence"], "compiler evidence, not certification");
    let rows: Vec<String> = v["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            format!(
                "{} {} {}:{} {}",
                o["function"].as_str().unwrap(),
                o["kind"].as_str().unwrap(),
                o["line"],
                o["column"],
                o["status"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "half precondition 2:5 checked",
            "half postcondition 3:5 checked",
            "half overflow 3:20 checked",
            "half division 4:5 proved",
            "ap indirect-call 5:48 unknown",
            "main call-precondition 6:20 proved",
            "main overflow 6:28 checked",
            "main index 6:30 checked",
            "main overflow 6:47 checked",
            "main overflow 6:51 proved",
            "main overflow 6:55 checked",
        ]
    );
    assert_eq!(
        (
            v["proved"].as_u64(),
            v["checked"].as_u64(),
            v["unknown"].as_u64()
        ),
        (Some(3), Some(7), Some(1))
    );
    // The same bytes every time.
    let again = nazm(d.path(), &["obligations", "--json", "prog.nz"]);
    assert_eq!(again.stdout, o.stdout);
}

#[test]
fn critical_and_cyber_refuse_an_obligation_of_unknown_status() {
    let src = "fn twice(x: Int) -> Int ! {} { x * 2 }\n\
               fn ap(f: fn(Int) -> Int, x: Int) -> Int ! {} { f(x) }\n\
               fn main() -> Int ! {} { ap(twice, 4) }\n";
    let d = program(&[("prog.nz", src)]);
    for profile in ["critical", "cyber"] {
        let o = nazm(d.path(), &["check", "--profile", profile, "prog.nz"]);
        assert!(!o.status.success());
        assert!(
            err(&o).contains("rule `no-unknown-calls`")
                && err(&o).contains("calls a function value"),
            "{profile}: {}",
            err(&o)
        );
    }
    // Called by name, every obligation is proved or checked, and the rule passes.
    std::fs::write(
        d.path().join("prog.nz"),
        "fn twice(x: Int) -> Int ! {} { x * 2 }\nfn main() -> Int ! {} { twice(4) }\n",
    )
    .unwrap();
    let o = nazm(d.path(), &["check", "--profile", "critical", "prog.nz"]);
    assert!(!err(&o).contains("no-unknown-calls"), "{}", err(&o));
}

#[test]
fn an_imported_function_s_preconditions_reach_its_callers_through_the_interface() {
    let d = program(&[
        ("nazm.root", ""),
        (
            "lib.nz",
            "pub fn half(n: Int) -> Int ! {}\n    requires n >= 0\n    requires n % 2 == 0\n{ n / 2 }\n",
        ),
        (
            "prog.nz",
            "use \"lib.nz\";\nfn main() -> Int { half(str_len(\"abcd\")) }\n",
        ),
    ]);
    let i = nazm(d.path(), &["interface", "lib.nz"]);
    assert!(out(&i).contains("\"requires\":2"), "{}", out(&i));
    for _ in 0..2 {
        // The second reads `lib.nz`'s interface back from the cache.
        let o = nazm(d.path(), &["obligations", "--json", "prog.nz"]);
        assert!(o.status.success(), "{}", err(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        let main_calls = v["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["function"] == "main" && o["kind"] == "call-precondition")
            .count();
        assert_eq!(main_calls, 1, "{v}");
    }
    // And the call runs against them.
    let r = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    assert_eq!(out(&r), "2\n");
}

#[test]
fn contract_clauses_are_formatted_as_the_header_s() {
    let d = program(&[(
        "prog.nz",
        "fn f(x: Int) -> Int ! {}\nrequires x > 0\n        ensures result > x\n    {\n    x + 1\n}\n",
    )]);
    let o = nazm(d.path(), &["fmt", "prog.nz"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        std::fs::read_to_string(d.path().join("prog.nz")).unwrap(),
        "fn f(x: Int) -> Int ! {}\n    requires x > 0\n    ensures result > x\n{\n    x + 1\n}\n"
    );
}

#[test]
fn the_representative_programs_run_alike_and_their_obligations_are_known() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/contracts");
    let isqrt = everywhere(&std::fs::read_to_string(root.join("isqrt.nz")).unwrap());
    assert_eq!(
        (isqrt.code, isqrt.stdout.as_str()),
        (Some(0), "0 9 3037000499\n0\n")
    );
    let ledger = everywhere(&std::fs::read_to_string(root.join("ledger.nz")).unwrap());
    assert_eq!((ledger.code, ledger.stdout.as_str()), (Some(0), "150\n0\n"));
    let o = Command::new(NAZM)
        .args(["obligations", "--json"])
        .arg(root.join("ledger.nz"))
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["unknown"], 0, "{v}");
}
