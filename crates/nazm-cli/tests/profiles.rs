//! N47: restriction profiles.
//!
//! `docs/architecture.md` §7.49. One language, four profiles: `general` restricts nothing,
//! `embedded` forbids the effects `io` and `spawn`, `critical` requires declared effect sets and a
//! locked build and forbids `spawn` and `foreign`, `cyber` requires declared effect sets and a
//! locked build and forbids `foreign`. Each rule reads a semantic fact, reports its witnesses,
//! and only refuses: a program a profile accepts runs exactly as under `general`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(cwd)
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

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// The rules a profile refused `src` for, from its report.
fn verdicts(src: &str, profile: &str) -> (bool, Vec<(String, String)>) {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", src);
    let o = nazm(
        d.path(),
        &["check", "--profile", profile, "--profile-report", "prog.nz"],
    );
    let report = out(&o)
        .lines()
        .find(|l| l.contains("nazm.profile-report/1"))
        .unwrap_or_else(|| panic!("no report: {}{}", out(&o), err(&o)))
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();
    // The report's outcome is the command's: a tool reading it is told what the exit says.
    assert_eq!(v["outcome"] == "accepted", o.status.success(), "{report}");
    let rules = v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_owned(),
                r["verdict"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    (o.status.success(), rules)
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

const AMBIENT: &str = "fn w(c: Chan) -> Int { chan_send(c, 1); 0 }\n\
                       fn show(io: IoCap, n: Int) -> Int { print(int_to_str(n)) }\n\
                       fn main(io: IoCap, t: SpawnCap) -> Int { let c = chan_new(1); scope { spawn w(c); } show(io, 1); 0 }\n";

const PURE: &str = "fn sq(n: Int) -> Int ! {} { n * n }\nfn main() -> Int ! {} { sq(7) }\n";

const DECLARED_TASKS: &str = "fn w(c: Chan) -> Int ! {} { chan_send(c, 1); 0 }\n\
                              fn main(t: SpawnCap) -> Int ! { spawn } { let c = chan_new(1); scope { spawn w(c); } 0 }\n";

#[test]
fn every_profile_s_rules_and_verdicts() {
    assert_eq!(verdicts(AMBIENT, "general"), (true, vec![]));
    assert_eq!(
        verdicts(AMBIENT, "embedded"),
        (
            false,
            pairs(&[
                ("no-io", "fail"),
                ("no-spawn", "fail"),
                ("bounded-allocation", "pass"),
                ("no-recursion", "pass")
            ])
        )
    );
    assert_eq!(
        verdicts(AMBIENT, "critical"),
        (
            false,
            pairs(&[
                ("declared-effects", "fail"),
                ("no-spawn", "fail"),
                ("no-foreign", "pass"),
                ("locked-build", "fail"),
                ("no-declassification", "pass"),
                ("no-ambient-time", "pass"),
                ("no-select", "pass"),
                ("no-unknown-calls", "pass")
            ])
        )
    );
    assert_eq!(
        verdicts(AMBIENT, "cyber"),
        (
            false,
            pairs(&[
                ("declared-effects", "fail"),
                ("no-foreign", "pass"),
                ("locked-build", "fail"),
                ("contents-stay-in-files", "pass"),
                ("checked-paths", "pass"),
                ("no-unknown-calls", "pass")
            ])
        )
    );
    // A pure, declared program passes everything that is about the program; only the build
    // rule is left, and it is about how the program was built.
    assert_eq!(
        verdicts(PURE, "embedded"),
        (
            true,
            pairs(&[
                ("no-io", "pass"),
                ("no-spawn", "pass"),
                ("bounded-allocation", "pass"),
                ("no-recursion", "pass")
            ])
        )
    );
    // `cyber` allows tasks; `critical` does not.
    let (_, cy) = verdicts(DECLARED_TASKS, "cyber");
    assert!(
        cy.contains(&("declared-effects".to_owned(), "pass".to_owned())),
        "{cy:?}"
    );
    let (_, cr) = verdicts(DECLARED_TASKS, "critical");
    assert!(
        cr.contains(&("no-spawn".to_owned(), "fail".to_owned())),
        "{cr:?}"
    );
}

#[test]
fn a_refusal_names_the_profile_the_rule_and_the_chain_of_calls() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "prog.nz",
        "fn deep(io: IoCap) -> Int ! { io } { print(\"x\") }\n\
         fn mid(io: IoCap) -> Int ! { io } { deep(io) }\n\
         fn main(io: IoCap) -> Int ! { io } { mid(io) }\n",
    );
    let o = nazm(d.path(), &["check", "--profile", "embedded", "prog.nz"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(e.contains("error[N0510]"), "{e}");
    assert!(e.contains("`embedded`") && e.contains("`no-io`"), "{e}");
    assert!(
        e.contains("`mid` declares `io`"),
        "the witness names the callee: {e}"
    );
}

#[test]
fn the_ambient_bridge_is_refused_by_the_strict_profiles() {
    // An undeclared function exercising its caller's authority is fine under `general`, and
    // refused wherever `declared-effects` is in force — named, each one.
    let (ok, _) = verdicts(AMBIENT, "general");
    assert!(ok);
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", AMBIENT);
    let o = nazm(d.path(), &["check", "--profile", "cyber", "prog.nz"]);
    let e = err(&o);
    for f in [
        "`w` declares no effect set",
        "`show` declares no effect set",
        "`main` declares no effect set",
    ] {
        assert!(e.contains(f), "{f}: {e}");
    }
}

#[test]
fn a_foreign_call_is_refused_where_no_foreign_is_in_force() {
    let src = "extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
               fn main(f: ForeignCap) -> Int ! { foreign } { c_add(1, 2) }\n";
    assert_eq!(
        verdicts(src, "embedded"),
        (
            true,
            pairs(&[
                ("no-io", "pass"),
                ("no-spawn", "pass"),
                ("bounded-allocation", "pass"),
                ("no-recursion", "pass")
            ])
        )
    );
    let (ok, cyber) = verdicts(src, "cyber");
    assert!(!ok);
    assert!(
        cyber.contains(&("no-foreign".to_owned(), "fail".to_owned())),
        "{cyber:?}"
    );
}

#[test]
fn a_standard_wrapper_carries_its_effect_into_the_profile() {
    // `io_println` is Nazm source: the profile sees `io` through it, not by its name.
    let src = "use \"@std/io\";\nfn main(io: IoCap) -> Int ! { io } { io_println(io, \"hi\") }\n";
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", src);
    let o = nazm(d.path(), &["check", "--profile", "embedded", "prog.nz"]);
    let e = err(&o);
    assert!(
        e.contains("`no-io`") && e.contains("`io_println` declares `io`"),
        "{e}"
    );
    // A pure standard function that allocates nothing is no obstacle.
    let pure = "use \"@std/seq\";\nfn main() -> Int ! {} { ints_sum(ints_new()) }\n";
    assert!(verdicts(pure, "embedded").0);
    // One that allocates in a loop is, since N52: what it allocates is not bounded.
    let trims = "use \"@std/text\";\nfn main() -> Int ! {} { str_len(text_trim(\" a \")) }\n";
    let (ok, rules) = verdicts(trims, "embedded");
    assert!(!ok, "{rules:?}");
    assert!(
        rules.contains(&("bounded-allocation".to_owned(), "fail".to_owned())),
        "{rules:?}"
    );
}

fn package(root: &Path, dep_profile: Option<&str>) {
    let profile = dep_profile.map_or(String::new(), |p| format!("profile = \"{p}\"\n"));
    write(
        root,
        "lib/nazm.toml",
        &format!("schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\n{profile}"),
    );
    write(
        root,
        "lib/src/lib.nz",
        "pub fn twice(n: Int) -> Int ! {} { n * 2 }\n",
    );
    write(
        root,
        "app/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\n[dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
    );
    write(
        root,
        "app/src/main.nz",
        "use \"lib:lib.nz\";\nfn main(io: IoCap) -> Int ! { io } { print(int_to_str(twice(21))) }\n",
    );
}

#[test]
fn a_locked_package_build_satisfies_the_build_rule_and_a_tampered_one_does_not() {
    let d = tempfile::tempdir().unwrap();
    package(d.path(), None);
    let o = nazm(d.path(), &["check", "--profile", "cyber", "app"]);
    assert!(err(&o).contains("`locked-build`"), "{}", err(&o));
    assert!(nazm(d.path(), &["lock", "app"]).status.success());
    let o = nazm(
        d.path(),
        &["run", "--interpret", "--profile", "cyber", "app"],
    );
    assert_eq!(out(&o), "422\n", "{}", err(&o));
    write(
        d.path(),
        "lib/src/lib.nz",
        "pub fn twice(n: Int) -> Int ! {} { n * 3 }\n",
    );
    let o = nazm(
        d.path(),
        &["run", "--interpret", "--profile", "cyber", "app"],
    );
    assert!(
        !o.status.success() && err(&o).contains("`locked-build`"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_dependency_s_required_profile_holds_the_whole_program() {
    // `lib` requires `embedded`; the program prints, so the build is refused however it is
    // asked for — and names both reasons for the profile.
    let d = tempfile::tempdir().unwrap();
    package(d.path(), Some("embedded"));
    let o = nazm(d.path(), &["check", "app"]);
    assert!(!o.status.success(), "{}", out(&o));
    assert!(
        err(&o).contains("`embedded`") && err(&o).contains("`no-io`"),
        "{}",
        err(&o)
    );
    // And an unknown one is refused by name, whoever wrote it.
    let e = tempfile::tempdir().unwrap();
    package(e.path(), Some("strict"));
    let o = nazm(e.path(), &["check", "app"]);
    assert!(
        err(&o).contains("N0511") && err(&o).contains("`lib`"),
        "{}",
        err(&o)
    );
    let o = nazm(
        e.path(),
        &["check", "--profile", "bogus", "app/src/main.nz"],
    );
    assert!(err(&o).contains("N0511"), "{}", err(&o));
}

#[test]
fn a_profile_refuses_and_never_changes_what_runs() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", PURE);
    let general = nazm(d.path(), &["run", "--interpret", "prog.nz"]);
    let embedded = nazm(
        d.path(),
        &["run", "--interpret", "--profile", "embedded", "prog.nz"],
    );
    assert_eq!(out(&general), out(&embedded));
    assert_eq!(out(&general), "49\n");
    // And a profile is evaluated every time: a check cached under `general` is no reason to
    // accept under `embedded`.
    write(d.path(), "nazm.root", "");
    write(d.path(), "prog.nz", AMBIENT);
    assert!(nazm(d.path(), &["check", "prog.nz"]).status.success());
    assert!(nazm(d.path(), &["check", "prog.nz"]).status.success());
    assert!(
        !nazm(d.path(), &["check", "--profile", "embedded", "prog.nz"])
            .status
            .success()
    );
    assert!(!nazm(
        d.path(),
        &["build", "--profile", "embedded", "prog.nz", "-o", "x"]
    )
    .status
    .success());
    assert!(!d.path().join(crate::common::exe_name("x")).exists());
}

#[test]
fn the_report_is_the_same_bytes_every_time() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", AMBIENT);
    let a = out(&nazm(
        d.path(),
        &[
            "check",
            "--profile",
            "critical",
            "--profile-report",
            "prog.nz",
        ],
    ));
    let b = out(&nazm(
        d.path(),
        &[
            "check",
            "--profile",
            "critical",
            "--profile-report",
            "prog.nz",
        ],
    ));
    assert_eq!(a, b);
    assert!(a.contains("not a certification"), "{a}");
}
