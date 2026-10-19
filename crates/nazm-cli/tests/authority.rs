//! N79: effects and capabilities v3 — the bridge as a recorded contract, attenuation,
//! subsumption.
//!
//! `docs/internal/architecture.md` §7.80. What the commands show: an `OutCap` authorises the standard
//! streams and nothing else; an `IoCap` is accepted where an `OutCap` is expected, never the
//! reverse; a function value whose effects are a subset of the expected ones is accepted, passed
//! or returned, and one with more is refused; every function's inherited authority is recorded
//! and shown by `nazm inspect`; and the `authority` profile refuses any function that inherits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn program(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    d
}

fn nazm(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn an_out_cap_prints_and_authorises_nothing_else() {
    let d = program(
        "fn shout(o: OutCap) -> Int ! {io} { print(\"loud\\n\"); eprint(\"err\\n\"); 1 }\n\
         fn main(io: IoCap) -> Int ! {io} { shout(io) }\n",
    );
    let o = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "loud\n1\n");

    for call in [
        "write_file(\"x\", \"y\")",
        "read_file(\"x\")",
        "file_exists(\"x\")",
    ] {
        let d = program(&format!(
            "fn log(o: OutCap) -> Int ! {{io}} {{ let r = {call}; 0 }}\n\
             fn main(io: IoCap) -> Int ! {{io}} {{ log(io) }}\n"
        ));
        let o = nazm(d.path(), &["check", "p.nz"]);
        let t = text(&o);
        assert!(!o.status.success(), "{call}: {t}");
        assert!(t.contains("N0369"), "{call}: {t}");
        assert!(t.contains("holds no `IoCap`"), "{call}: {t}");
    }
}

#[test]
fn an_io_cap_is_accepted_for_an_out_cap_and_never_the_reverse() {
    let d = program(
        "fn wide(i: IoCap) -> Int ! {io} { 0 }\n\
         fn main(o: OutCap) -> Int ! {io} { wide(o) }\n",
    );
    let o = nazm(d.path(), &["check", "p.nz"]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(
        t.contains("N0300") && t.contains("is `IoCap`, but this is `OutCap`"),
        "{t}"
    );

    // Nothing makes an `IoCap` from an `OutCap`, returned either.
    let d = program(
        "fn widen(o: OutCap) -> IoCap ! {} { o }\n\
         fn main(o: OutCap) -> Int ! {io} { 0 }\n",
    );
    let o = nazm(d.path(), &["check", "p.nz"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("N03"), "{}", text(&o));
}

#[test]
fn main_may_hold_only_an_out_cap_on_every_engine() {
    let d = program(
        "fn inc(n: Int) -> Int ! {} { n + 1 }\n\
         fn main(o: OutCap) -> Int ! {io} { print(\"hi\\n\"); inc(41) }\n",
    );
    let o = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n42\n");
    let o = nazm(
        d.path(),
        &["build", "--backend", "cranelift", "p.nz", "-o", "p"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let o = Command::new(d.path().join(crate::common::exe_name("p")))
        .output()
        .unwrap();
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n42\n");
}

/// An `IoCap` handed where an `OutCap` is expected compiles natively, on both backends, and
/// runs as the interpreter does — to a generic function too. Until N104 the first
/// type-checked and then failed MIR validation (`N0900`: MIR's copy of the shape relation had
/// no rule for capabilities), and the second was refused (`N0300`: a generic call compared
/// its concrete parameters exactly).
#[test]
fn an_io_cap_given_for_an_out_cap_compiles_on_both_backends() {
    let d = program(
        "fn say(out: OutCap, n: Int) -> Int { print(int_to_str(n)); n }\n\
         fn twice[T](x: T, out: OutCap) -> Int { print(\"!\"); 0 }\n\
         fn main(io: IoCap) -> Int { say(io, 7) - 7 + twice(true, io) }\n",
    );
    let o = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    // `print` adds no newline; `nazm run` then prints `main`'s result.
    assert_eq!(String::from_utf8_lossy(&o.stdout), "7!0\n");
    for backend in ["llvm", "cranelift"] {
        let o = nazm(
            d.path(),
            &["build", "--backend", backend, "p.nz", "-o", "p"],
        );
        assert!(o.status.success(), "{backend}: {}", text(&o));
        let o = Command::new(d.path().join(crate::common::exe_name("p")))
            .output()
            .unwrap();
        assert!(o.status.success(), "{backend}");
        assert_eq!(String::from_utf8_lossy(&o.stdout), "7!0\n", "{backend}");
    }
}

#[test]
fn an_out_cap_cannot_be_defined_by_a_program() {
    let d = program("struct OutCap { x: Int }\nfn main() -> Int { 0 }\n");
    let o = nazm(d.path(), &["check", "p.nz"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("N0331"), "{}", text(&o));
}

#[test]
fn a_function_value_with_fewer_effects_is_accepted_passed_or_returned() {
    let d = program(
        "fn apply(f: fn(Int) -> Int ! {io}, x: Int) -> Int ! {io} { f(x) }\n\
         fn inc(n: Int) -> Int ! {} { n + 1 }\n\
         fn pick() -> fn(Int) -> Int ! {io} { inc }\n\
         fn main(io: IoCap) -> Int ! {io} { let f = pick(); apply(inc, 40) + f(0) }\n",
    );
    let o = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "42\n");
    // Below the checker nothing moved: the verifiers compare up to effects, so it builds.
    let o = nazm(
        d.path(),
        &["build", "--backend", "cranelift", "p.nz", "-o", "p"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let o = Command::new(d.path().join(crate::common::exe_name("p")))
        .output()
        .unwrap();
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout), "42\n");
}

#[test]
fn a_function_value_with_more_effects_is_refused() {
    for (what, src) in [
        (
            "passed",
            "fn apply(f: fn(Int) -> Int ! {}, x: Int) -> Int { f(x) }\n\
             fn loud(n: Int) -> Int ! {io} { n }\n\
             fn main(io: IoCap) -> Int ! {io} { apply(loud, 1) }\n",
        ),
        (
            "returned",
            "fn loud(n: Int) -> Int ! {io} { n }\n\
             fn pick() -> fn(Int) -> Int ! {} { loud }\n\
             fn main() -> Int { 0 }\n",
        ),
    ] {
        let d = program(src);
        let o = nazm(d.path(), &["check", "p.nz"]);
        let t = text(&o);
        assert!(!o.status.success(), "{what}: {t}");
        assert!(t.contains("N0300") || t.contains("N03"), "{what}: {t}");
    }
}

#[test]
fn nothing_inherits_authority_and_inspect_reports_no_inheritance() {
    // N104: a function that declares no effect set no longer exercises its caller's authority.
    // It holds what it exercises, or it is refused at the first site that needs it. (Writing a
    // file: since D8 every function of the program holds the output authority `main` holds, so
    // printing no longer shows it; an `IoCap` is still only ever passed.)
    let d = program(
        "fn helper() -> Int { write_file(\"f\", \"x\") }\n\
         fn main(io: IoCap) -> Int ! {io} { helper() }\n",
    );
    let o = nazm(d.path(), &["check", "p.nz"]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(
        t.contains("N0369") && t.contains("`helper` holds no `IoCap`"),
        "{t}"
    );
    // Given one, it is accepted; and `nazm inspect`, now `/2`, reports no inheritance at all.
    let d = program(
        "fn helper(io: IoCap) -> Int { write_file(\"f\", \"x\") }\n\
         fn pure(n: Int) -> Int { n + 1 }\n\
         fn main(io: IoCap) -> Int ! {io} { helper(io) + pure(0) }\n",
    );
    let o = nazm(d.path(), &["inspect", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    let doc: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["schema"], "nazm.inspect/2");
    for def in doc["definitions"].as_array().unwrap() {
        assert!(def.get("inherits").is_none(), "{def}");
    }
}

#[test]
fn the_authority_profile_and_the_default_refuse_the_same_programs() {
    // N104: `explicit-authority` is the language's rule now. A function exercising what it
    // does not hold is refused by the default (`N0369`) and the profile alike; the profile's
    // own rule has nothing left to refuse. (A file write: printing in the program needs
    // nothing passed since D8.)
    let d = program(
        "fn helper() -> Int { write_file(\"f\", \"x\") }\n\
         fn main(io: IoCap) -> Int ! {io} { helper() }\n",
    );
    for args in [
        &["check", "p.nz"][..],
        &["check", "--profile", "authority", "p.nz"],
    ] {
        let o = nazm(d.path(), args);
        let t = text(&o);
        assert!(!o.status.success(), "{args:?}: {t}");
        assert!(t.contains("N0369"), "{args:?}: {t}");
        assert!(!t.contains("explicit-authority"), "{args:?}: {t}");
    }
    // Held authority, and a function that exercises none, are accepted by both.
    let d = program(
        "fn helper(io: IoCap) -> Int { write_file(\"f\", \"x\") }\n\
         fn pure(n: Int) -> Int { n + 1 }\n\
         fn main(io: IoCap) -> Int ! {io} { helper(io) + pure(1) }\n",
    );
    for args in [
        &["check", "p.nz"][..],
        &["check", "--profile", "authority", "p.nz"],
    ] {
        let o = nazm(d.path(), args);
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
}

#[test]
fn the_profile_report_names_the_rule() {
    let d = program(
        "fn helper(out: OutCap) -> Int { print(\"x\\n\"); 0 }\n\
         fn main(io: IoCap) -> Int ! {io} { helper(io) }\n",
    );
    let o = nazm(
        d.path(),
        &[
            "check",
            "--profile",
            "authority",
            "--profile-report",
            "p.nz",
        ],
    );
    let t = String::from_utf8_lossy(&o.stdout);
    assert!(t.contains("\"explicit-authority\""), "{t}");
}

#[test]
fn a_held_io_cap_covers_what_an_undeclared_helper_prints() {
    // `IoCap` implies `OutCap` (N79): a helper with no declared effects that prints is called
    // under the caller's `IoCap` without a separate `OutCap`.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.nz"),
        "fn say(out: OutCap) -> Int { print(\"hi\"); 0 }\nfn main(io: IoCap) -> Int ! { io } { say(io) }\n",
    )
    .unwrap();
    let o = nazm(d.path(), &["check", "a.nz"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_held_io_cap_authorises_its_own_print() {
    // N101: the case N100 missed. A declared function holding only an `IoCap` prints itself —
    // `print` requires an `OutCap`, which the held `IoCap` implies. Run, not only checked, and
    // in a fresh directory: a check cache keyed by the semantic epoch alone would answer for an
    // earlier build of the checker.
    let d = program(
        "fn say(io: IoCap) -> Int ! {io} { print(\"hi\\n\"); eprint(\"e\\n\"); 2 }\n\
         fn main(io: IoCap) -> Int ! {io} { say(io) }\n",
    );
    let o = nazm(d.path(), &["run", "--interpret", "p.nz"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n2\n");
    // The implication is one way: an `OutCap` gives no `IoCap`.
    let d = program(
        "fn say(o: OutCap) -> Int ! {io} { exit_with(1); 0 }\n\
         fn main(io: IoCap) -> Int ! {io} { say(io) }\n",
    );
    let o = nazm(d.path(), &["check", "p.nz"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("N0369"), "{t}");
}
