//! N60: restriction profiles v2 — stack and determinism rules, and the `unknown` verdict.
//!
//! `docs/internal/architecture.md` §7.62. Each rule is read off compiler facts — MIR's stack checks and
//! indirect calls, the resolution's call sites — and its verdict is `pass`, `fail` or `unknown`,
//! with `unknown` refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

/// `(accepted, [(rule, verdict)])` from `nazm.profile-report/1`, with the refusal's stderr.
fn verdicts(src: &str, profile: &str) -> (bool, Vec<(String, String)>, String) {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", src);
    let o = nazm(
        d.path(),
        &["check", "--profile", profile, "--profile-report", "prog.nz"],
    );
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    let line = stdout
        .lines()
        .find(|l| l.contains("nazm.profile-report/1"))
        .unwrap_or_else(|| panic!("no report: {stdout}"))
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["outcome"] == "accepted", o.status.success(), "{line}");
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
    (
        o.status.success(),
        rules,
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn verdict_of(rules: &[(String, String)], id: &str) -> String {
    rules
        .iter()
        .find(|(r, _)| r == id)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

#[test]
fn embedded_refuses_a_call_cycle_and_names_each_function_in_it() {
    let direct = "fn count(n: Int) -> Int ! {} { if n == 0 { 0 } else { count(n - 1) + 1 } }\n\
                  fn main() -> Int ! {} { count(3) }\n";
    let (ok, rules, err) = verdicts(direct, "embedded");
    assert!(!ok);
    assert_eq!(verdict_of(&rules, "no-recursion"), "fail", "{rules:?}");
    assert!(err.contains("`count` is in a call cycle"), "{err}");
    let mutual = "fn even(n: Int) -> Bool ! {} { if n == 0 { true } else { odd(n - 1) } }\n\
                  fn odd(n: Int) -> Bool ! {} { if n == 0 { false } else { even(n - 1) } }\n\
                  fn main() -> Int ! {} { if even(4) { 1 } else { 0 } }\n";
    let (ok, rules, err) = verdicts(mutual, "embedded");
    assert!(!ok);
    assert_eq!(verdict_of(&rules, "no-recursion"), "fail", "{rules:?}");
    assert!(
        err.contains("`even` is in a call cycle") && err.contains("`odd`"),
        "{err}"
    );
    // No cycle: a stack bound exists.
    let flat = "fn sq(n: Int) -> Int ! {} { n * n }\nfn main() -> Int ! {} { sq(sq(2)) }\n";
    let (ok, rules, _) = verdicts(flat, "embedded");
    assert!(ok, "{rules:?}");
    assert_eq!(verdict_of(&rules, "no-recursion"), "pass");
}

/// Q1-C-01: an allocation in a loop is refused however it is reached — a helper, a trait method,
/// a generic — because a call of a function that may allocate is an allocation site of its own.
/// Allocating once outside a loop, or taking a view in one, stays accepted.
#[test]
fn bounded_allocation_sees_an_allocation_behind_a_call() {
    let lp = "fn main() -> Int ! {} { let mut i = 0; let mut t = 0; while i < 3 { t = t + CALL; i = i + 1; } t }\n";
    for (what, defs, call) in [
        ("a helper", "fn mk(i: Int) -> Int ! {} { let xs = ints_new(); ints_push(xs, i); ints_len(xs) }\n", "mk(i)"),
        ("a helper's helper", "fn mk2(i: Int) -> Int ! {} { let xs = ints_new(); ints_push(xs, i); ints_len(xs) }\nfn mk(i: Int) -> Int ! {} { mk2(i) }\n", "mk(i)"),
        ("a trait method", "trait A { fn mk(self: Self) -> Int ! {}; }\nimpl A for Int { fn mk(self: Int) -> Int ! {} { let xs = ints_new(); ints_push(xs, self); ints_len(xs) } }\n", "i.mk()"),
        ("a generic", "fn mk[X](x: X) -> Int ! {} { let v = vec_new[X](); vec_push(v, x); vec_len(v) }\n", "mk(i)"),
    ] {
        let src = format!("{defs}{}", lp.replace("CALL", call));
        let (ok, rules, err) = verdicts(&src, "embedded");
        assert!(!ok, "{what}: {rules:?}");
        assert_eq!(verdict_of(&rules, "bounded-allocation"), "fail", "{what}: {rules:?}");
        assert!(err.contains("call of `"), "{what}: {err}");
    }
    let once =
        "fn mk(i: Int) -> Int ! {} { let xs = ints_new(); ints_push(xs, i); ints_len(xs) }\n\
                fn main() -> Int ! {} { mk(1) }\n";
    let (ok, rules, _) = verdicts(once, "embedded");
    assert!(ok, "{rules:?}");
    assert_eq!(verdict_of(&rules, "bounded-allocation"), "pass");
}

/// Q1-C-02: every built-in that makes storage is an allocation site, Gate 2's included. A view
/// (`bytes_slice`, `text_slice`) shares an owner and is not.
#[test]
fn bounded_allocation_counts_every_built_in_that_allocates() {
    let lp = |body: &str| {
        format!(
            "fn main() -> Int ! {{}} {{ let b = bytes_new(4); let s = \"ab\"; let mut i = 0; let mut t = 0; \
             while i < 3 {{ t = t + {body}; i = i + 1; }} t }}\n"
        )
    };
    for body in [
        "bytes_len(bytes_new(8))",
        "bytes_len(bytes_from_str(s))",
        "str_len(bytes_to_str(b))",
        "str_len(num_to_str(i))",
        "text_len(text_concat(Text(s), Text(s)))",
        "text_len(scalar_text(97))",
    ] {
        let (ok, rules, _) = verdicts(&lp(body), "embedded");
        assert!(!ok, "{body}: {rules:?}");
        assert_eq!(verdict_of(&rules, "bounded-allocation"), "fail", "{body}");
    }
    for body in [
        "bytes_len(bytes_slice(b, 0, 2))",
        "text_len(text_slice(Text(s), 0, 1))",
    ] {
        let (ok, rules, _) = verdicts(&lp(body), "embedded");
        assert!(ok, "{body}: {rules:?}");
    }
}

/// Q1-C-04: a `nazm.root` marker beside a package's entry does not take the package out from
/// under its manifest. The manifest's profile holds for `check DIR`, `run DIR`, `build DIR` and
/// for the entry file named directly.
#[test]
fn a_root_marker_inside_a_package_does_not_drop_its_profile() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "pkg/nazm.toml",
        "schema = \"nazm.package/1\"\n\n[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\nprofile = \"embedded\"\n",
    );
    write(
        d.path(),
        "pkg/src/main.nz",
        "fn main(o: OutCap) -> Int ! { io } { print(\"io\\n\"); 0 }\n",
    );
    write(d.path(), "pkg/src/nazm.root", "");
    for args in [
        vec!["check", "pkg"],
        vec!["run", "--interpret", "pkg"],
        vec!["build", "pkg", "-o", "out"],
        vec!["check", "pkg/src/main.nz"],
    ] {
        let o = nazm(d.path(), &args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{args:?}: {err}");
        assert!(
            err.contains("N0510") && err.contains("no-io"),
            "{args:?}: {err}"
        );
    }
    assert!(!d.path().join("out").exists());
}

#[test]
fn a_call_through_a_function_value_is_unknown_and_unknown_refuses() {
    let src = "fn apply(f: fn(Int) -> Int ! {}, x: Int) -> Int ! {} { f(x) }\n\
               fn inc(x: Int) -> Int ! {} { x + 1 }\n\
               fn main() -> Int ! {} { apply(inc, 1) }\n";
    let (ok, rules, err) = verdicts(src, "embedded");
    assert!(!ok, "unknown is not a pass: {rules:?}");
    assert_eq!(verdict_of(&rules, "no-recursion"), "unknown", "{rules:?}");
    assert!(err.contains("N0510") && err.contains("unknown:"), "{err}");
    assert!(err.contains("`apply` calls a function value here"), "{err}");
    // `general` has no rules; the same program is accepted there.
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "prog.nz", src);
    assert!(nazm(d.path(), &["check", "prog.nz"]).status.success());
}

#[test]
fn critical_refuses_a_select_by_its_call_site() {
    let src =
        "fn main() -> Int ! {} {\n    let c = chan_new_of[Int](1);\n    chan_send_of(c, 1);\n    \
               let cs = vec_new[Chan[Int]]();\n    vec_push(cs, c);\n    \
               chan_select_of(cs, vec_new[Int]())\n}\n";
    let (ok, rules, err) = verdicts(src, "critical");
    assert!(!ok);
    assert_eq!(verdict_of(&rules, "no-select"), "fail", "{rules:?}");
    assert!(
        err.contains("depends on which channel is ready first"),
        "{err}"
    );
    // A receive from one channel is allowed: its order is the program's.
    let one = "fn main() -> Int ! {} {\n    let c = chan_new_of[Int](1);\n    chan_send_of(c, 1);\n    \
               let got = vec_new[Int]();\n    if chan_recv_of(c, got) { vec_pop(got) } else { 0 }\n}\n";
    let (_, rules, _) = verdicts(one, "critical");
    assert_eq!(verdict_of(&rules, "no-select"), "pass", "{rules:?}");
}

#[test]
fn a_dependency_declaring_general_does_not_weaken_the_root_s_profile() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "lib/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"lib\"\nversion = \"1.0.0\"\nprofile = \"general\"\n",
    );
    write(
        d.path(),
        "lib/src/lib.nz",
        "pub fn spin(n: Int) -> Int ! {} { if n == 0 { 0 } else { spin(n - 1) } }\n",
    );
    write(
        d.path(),
        "app/nazm.toml",
        "schema = \"nazm.package/1\"\n[package]\nname = \"app\"\nversion = \"0.1.0\"\nmain = \"main.nz\"\nprofile = \"embedded\"\n\
         [dependencies]\nlib = { path = \"../lib\", version = \"1.0.0\" }\n",
    );
    write(
        d.path(),
        "app/src/main.nz",
        "use \"lib:lib.nz\";\nfn main() -> Int ! {} { spin(3) }\n",
    );
    let o = nazm(d.path(), &["check", "app"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        !o.status.success(),
        "the root's embedded still holds: {err}"
    );
    assert!(
        err.contains("`no-recursion`") && err.contains("`spin` is in a call cycle"),
        "{err}"
    );
}

#[test]
fn every_command_offering_a_profile_names_every_profile_in_its_help() {
    // M1: the `--profile` help named four of the eight profiles for three commands, written by
    // hand when there were four. The list is `Profile::ALL`; the help is held to it.
    let mut offered = 0;
    for cmd in ["check", "build", "run", "test", "inspect"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_nazm"))
            .args([cmd, "--help"])
            .output()
            .expect("run nazm");
        let help = String::from_utf8_lossy(&out.stdout);
        if !help.contains("--profile") {
            continue;
        }
        offered += 1;
        for p in nazm_service::profile::Profile::ALL {
            assert!(
                help.contains(&format!("`{}`", p.name())),
                "`nazm {cmd} --help` does not name the profile `{}`",
                p.name()
            );
        }
    }
    assert!(offered > 0, "no command offers --profile any more");
}
