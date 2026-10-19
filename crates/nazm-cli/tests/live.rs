//! N65: the interactive tier — `nazm repl`, `nazm comptime`, `nazm reload-check`.
//!
//! `docs/internal/architecture.md` §7.67. What the commands show end to end: a REPL session checks every
//! edit as one program, so a redefinition reaches its callers and one that breaks a caller is
//! refused by the caller's name; comptime evaluates a parameterless pure function and refuses any
//! other by name, and a runaway evaluation stops at its budget; a reload check says, per
//! definition, whether a running image could take the change. The JIT is BLOCKED (§7.67): no test
//! here stands in for it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn repl(input: &str) -> String {
    let mut child = Command::new(NAZM)
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn a_session_is_checked_as_one_program_on_every_edit() {
    let out = repl(
        "fn g(x: Int) -> Int { x + 1 }\n\
         fn f(x: Int) -> Int { g(x) * 2 }\n\
         f(3)\n\
         fn g(x: Int) -> Int { x + 10 }\n\
         f(3)\n\
         fn g(x: Bool) -> Int { 0 }\n\
         f(3)\n\
         f(3) == 26\n\
         \"text\"\n\
         :defs\n\
         :quit\n\
         f(4)\n",
    );
    let want = "defined `g`\n\
                defined `f`\n\
                = 8\n\
                redefined `g`\n\
                = 26\n\
                redefining `g` is refused: `f` no longer checks against it\n";
    assert!(out.starts_with(want), "{out}");
    // Refused, the session is unchanged: `f` still calls the second `g`.
    let after = &out[want.len()..];
    assert!(after.contains("= 26\n= true\ntext\n"), "{out}");
    assert!(
        after.contains("fn g(x: Int) -> Int { x + 10 }\nfn f(x: Int) -> Int { g(x) * 2 }\n"),
        "{out}"
    );
    // Nothing after `:quit` runs.
    assert!(!out.contains("= 10"), "{out}");
}

#[test]
fn a_failure_in_the_repl_is_the_language_s_failure() {
    let out = repl("9223372036854775807 + 1\nfn main() -> Int { 0 }\n");
    assert!(out.contains("error[N0400]"), "{out}");
    assert!(out.contains("`main` is the REPL's"), "{out}");
}

const CONSTANTS: &str = "fn table() -> Int ! {} { let mut i = 0; let mut s = 0; while i < 10 { s = s + i * i; i = i + 1; } s }\n\
                         fn reads(io: IoCap) -> Int ! { io } { print(\"x\"); 0 }\n\
                         fn loud() -> Int ! { io } { 0 }\n\
                         fn ambient(out: OutCap) -> Int { print(\"hi\"); 0 }\n\
                         fn forever() -> Int ! {} { let mut i = 0; while i >= 0 { i = i + 0; } i }\n\
                         fn main() -> Int { table() }\n";

#[test]
fn comptime_evaluates_a_pure_constant_and_refuses_anything_that_could_reach_the_host() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("c.nz"), CONSTANTS).unwrap();
    let o = nazm(d.path(), &["comptime", "c.nz", "table"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "285\n");
    for (f, why) in [
        ("reads", "it has parameters, so it is not a constant"),
        ("loud", "it is not pure"),
        // It exercises authority, so it takes it as a parameter (N104): comptime has none to
        // hand it, and a function with parameters is no constant.
        ("ambient", "it has parameters, so it is not a constant"),
        ("nothere", "there is no function `nothere`"),
    ] {
        let o = nazm(d.path(), &["comptime", "c.nz", f]);
        assert!(!o.status.success(), "{f} was evaluated");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains("N0393"), "{f}: {e}");
        assert!(e.contains(why), "{f}: {e}");
        assert!(o.stdout.is_empty(), "{f} printed");
    }
    // A runaway evaluation stops at its budget, with the interpreter's own code.
    let o = nazm(
        d.path(),
        &["comptime", "c.nz", "forever", "--max-iterations", "10000"],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("N0402"));
}

fn versions(d: &Path, sources: &[(&str, &str)]) {
    for (name, src) in sources {
        std::fs::create_dir_all(d.join(name)).unwrap();
        std::fs::write(d.join(name).join("prog.nz"), src).unwrap();
    }
}

#[test]
fn a_reload_check_names_what_a_running_image_can_take_and_what_needs_a_restart() {
    let d = tempfile::tempdir().unwrap();
    versions(
        d.path(),
        &[
            (
                "v1",
                "struct P { x: Int, y: Int }\nfn f(p: P) -> Int { p.x + p.y }\nfn main() -> Int { f(P(x: 1, y: 2)) }\n",
            ),
            // Fields reordered (not a layout change), a body edited, a function added.
            (
                "v2",
                "struct P { y: Int, x: Int }\nfn f(p: P) -> Int { p.x * p.y }\nfn g() -> Int { 4 }\nfn main() -> Int { f(P(x: 1, y: 2)) + g() }\n",
            ),
            (
                "v3",
                "struct P { x: Int, y: Int, z: Int }\nfn f(p: P, k: Int) -> Int { p.x + p.y + k }\nfn main() -> Int { f(P(x: 1, y: 2, z: 3), 1) }\n",
            ),
        ],
    );
    let o = nazm(d.path(), &["reload-check", "v1/prog.nz", "v2/prog.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["schema"], "nazm.reload-check/1");
    assert_eq!(v["outcome"], "reloadable");
    let names: Vec<(&str, &str)> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].as_str().unwrap(), c["verdict"].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        [
            ("f", "reloadable"),
            ("g", "reloadable"),
            ("main", "reloadable")
        ]
    );

    let o = nazm(d.path(), &["reload-check", "v1/prog.nz", "v3/prog.nz"]);
    assert!(!o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["outcome"], "restart");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains("`P` needs a restart: the record's layout changed"),
        "{e}"
    );
    assert!(
        e.contains("`f` needs a restart: its signature changed from `(P) -> Int ! {}` to `(P, Int) -> Int ! {}`"),
        "{e}"
    );

    // The same version against itself changes nothing.
    let o = nazm(d.path(), &["reload-check", "v1/prog.nz", "v1/prog.nz"]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn an_effect_added_to_a_signature_needs_a_restart() {
    let d = tempfile::tempdir().unwrap();
    versions(
        d.path(),
        &[
            ("a", "fn f() -> Int ! {} { 1 }\nfn main() -> Int { f() }\n"),
            (
                "b",
                "fn f(io: IoCap) -> Int ! { io } { print(\"x\"); 1 }\nfn main(io: IoCap) -> Int ! { io } { f(io) }\n",
            ),
        ],
    );
    let o = nazm(d.path(), &["reload-check", "a/prog.nz", "b/prog.nz"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("`f` needs a restart"));
}
