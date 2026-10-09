//! Semantic reuse: what is skipped, what is not, and where invalidation stops.
//!
//! # Why every one of these runs the binary
//!
//! Reuse is a claim about **two runs**. A test that calls a planner twice in one process
//! establishes that a data structure agrees with itself; it says nothing about whether a
//! result written by one compiler is found, validated and acted on by another. So each
//! test here runs `nazm check` as a program, repeatedly, and reads the decisions out of
//! its own report rather than inferring them from a timing.
//!
//! # The two claims kept apart
//!
//! * **Reuse happened** — the report says which modules were checked. Without this the
//!   suite would pass on a compiler that had no cache at all.
//! * **Reuse changed nothing** — every program here is also checked with `--no-cache`, and
//!   the two runs must agree on acceptance, on diagnostic codes and on positions. A cache
//!   may change the work done; it may not change what a program means.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    // Halving a length to truncate a file is arithmetic on bytes, not a measurement.
    clippy::integer_division
)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The core prelude: one module in every compilation, imported by every other.
///
/// Counted wherever modules are counted, because it is checked, cached and reused exactly
/// as a project module is — `docs/spec.md`, *The core prelude*. Named rather than folded
/// into each number, so every count below still reads as the project's modules plus it.
const PRELUDE: u64 = 1;

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// What one `nazm check` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    accepted: bool,
    /// Every diagnostic, as `CODE@line:column` — the part a cache may not change.
    diagnostics: Vec<String>,
    checked: u64,
    reused: u64,
    missed: u64,
    rejected: u64,
    declined: u64,
    stored: u64,
    root: String,
}

fn check_in(dir: &Path, file: &str, extra: &[&str]) -> Run {
    let out = Command::new(NAZM)
        .arg("check")
        .arg(file)
        .args(["--cache-report", "--json"])
        .args(extra)
        .current_dir(dir)
        .output()
        .expect("run nazm check");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value = stdout
        .lines()
        .find(|l| l.contains("nazm.check-report/1"))
        .map(|l| serde_json::from_str(l).expect("a report"))
        .expect("a cache report on stdout");

    let n = |k: &str| report[k].as_u64().expect("a number");
    Run {
        accepted: out.status.success(),
        diagnostics: String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            // The code, then the whole diagnostic — code, message, every span and label, help and fixes — so
            // a cache that changed a word of one is caught. Until Q1 this read
            // `primary.line` and `primary.column`, which the JSON does not have: every position
            // compared as `0:0` and no message was compared at all (Q1-M-09).
            .map(|d| format!("{} {d}", d["code"].as_str().unwrap_or("?")))
            .collect(),
        checked: n("checked"),
        reused: n("reused"),
        missed: n("missed"),
        rejected: n("rejected"),
        declined: n("declined"),
        stored: n("stored"),
        root: report["root"].as_str().unwrap_or("?").to_owned(),
    }
}

fn check(dir: &Path, file: &str) -> Run {
    check_in(dir, file, &[])
}

fn cold(dir: &Path, file: &str) -> Run {
    check_in(dir, file, &["--no-cache"])
}

/// What a cache may not change: the answer, and every diagnostic in it.
fn meaning(r: &Run) -> (bool, &[String]) {
    (r.accepted, &r.diagnostics)
}

/// The answer, and every diagnostic's code and spans, without its wording. Used only where a
/// warm run's wording is known to differ: Q1-M-08, a parameter of a module the cache reused is
/// named by position (`#1`), because its body's locals were never bound. Open, FIX-BEFORE-V1.
fn verdict(r: &Run) -> (bool, Vec<String>) {
    let spans = |d: &serde_json::Value| {
        let mut out = vec![d["code"].to_string(), d["primary"]["span"].to_string()];
        for s in d["secondary"].as_array().into_iter().flatten() {
            out.push(s["span"].to_string());
        }
        out.join(" ")
    };
    let diagnostics = r
        .diagnostics
        .iter()
        .map(|d| {
            let json = d.split_once(' ').map_or("", |(_, j)| j);
            serde_json::from_str::<serde_json::Value>(json)
                .map_or_else(|_| d.clone(), |v| spans(&v))
        })
        .collect();
    (r.accepted, diagnostics)
}

/// A project marked with a root, so its modules keep one identity.
fn project(dir: &Path) {
    write(dir, "nazm.root", "");
}

// --- the shape §33 and §34 of the directive ask for: A → B -------------------------------

fn two_modules(dir: &Path, b: &str) {
    project(dir);
    write(
        dir,
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { twice(21) }\n",
    );
    write(dir, "b.nz", b);
}

const B: &str = "pub fn twice(n: Int) -> Int { helper() * n }\nfn helper() -> Int { 2 }\n";

#[test]
fn a_second_run_over_an_unchanged_project_checks_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);

    let first = check(dir.path(), "a.nz");
    assert_eq!(
        (first.checked, first.reused, first.stored),
        (2 + PRELUDE, 0, 2 + PRELUDE)
    );
    assert_eq!(first.root, "marker");

    let second = check(dir.path(), "a.nz");
    assert_eq!((second.checked, second.reused), (0, 2 + PRELUDE));
    assert!(second.accepted);
}

#[test]
fn a_private_change_rechecks_the_module_and_leaves_its_importer_alone() {
    // The decisive test. B's private helper changes: B's source fingerprint moves, so B
    // is checked again; B's *interface* does not move, so A's key is untouched and A is
    // not. Invalidation is interface-driven, and this is where that stops being a claim.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);
    check(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { helper() * n }\nfn helper() -> Int { 1 + 1 }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        (after.checked, after.reused, after.missed),
        (1, 1 + PRELUDE, 1),
        "exactly B should have been checked"
    );
    assert!(after.accepted);

    // And the answer is the one a compilation with no cache at all reaches.
    let disabled = cold(dir.path(), "a.nz");
    assert_eq!(meaning(&after), meaning(&disabled));
}

#[test]
fn every_kind_of_private_change_leaves_the_importer_alone() {
    // §12 of the directive, one row at a time. Each edit moves B's source and not its
    // interface, so each rechecks B alone.
    let edits: &[(&str, &str)] = &[
        (
            "a private body changes",
            "pub fn twice(n: Int) -> Int { helper() * n }\nfn helper() -> Int { 2 }\n\
             fn unused() -> Int { 9 }\n",
        ),
        (
            "a private function is added",
            "pub fn twice(n: Int) -> Int { helper() * n }\nfn helper() -> Int { 2 }\n\
             fn extra() -> Int { 3 }\n",
        ),
        (
            "a private function is removed",
            "pub fn twice(n: Int) -> Int { 2 * n }\n",
        ),
        (
            "an exported body changes",
            "pub fn twice(n: Int) -> Int { n * helper() }\nfn helper() -> Int { 2 }\n",
        ),
        (
            "a comment is added",
            "// what it does\npub fn twice(n: Int) -> Int { helper() * n }\n\
             fn helper() -> Int { 2 }\n",
        ),
        (
            "the layout changes",
            "pub fn twice( n : Int ) -> Int {\n    helper() * n\n}\nfn helper() -> Int { 2 }\n",
        ),
    ];

    for (what, b) in edits {
        let dir = tempfile::tempdir().expect("tempdir");
        two_modules(dir.path(), B);
        check(dir.path(), "a.nz");

        write(dir.path(), "b.nz", b);
        let after = check(dir.path(), "a.nz");
        assert_eq!(
            (after.checked, after.reused),
            (1, 1 + PRELUDE),
            "{what}: the importer should not have been re-checked"
        );
        assert!(after.accepted, "{what}");
    }
}

#[test]
fn an_exported_signature_change_rechecks_the_importer() {
    // §34. B's interface moves, so A's key moves, so A is checked — and the error A now
    // has is found. A cache that got this wrong would accept a program that does not
    // type-check, which is the failure the whole milestone is arranged around.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted);

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Str) -> Int { str_len(n) }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (2, PRELUDE));
    assert!(!after.accepted, "the call in A no longer type-checks");
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
}

#[test]
fn every_kind_of_exported_change_rechecks_the_importer() {
    // §13, one row at a time. Each moves B's interface, so each moves A's key. The
    // programs differ in whether they still check; what is asserted is that A was asked.
    let edits: &[(&str, &str)] = &[
        (
            "a parameter type changes",
            "pub fn twice(n: Str) -> Int { str_len(n) }\n",
        ),
        (
            "the return type changes",
            "pub fn twice(n: Int) -> Str { int_to_str(n) }\n",
        ),
        ("pub is removed", "fn twice(n: Int) -> Int { n * 2 }\n"),
        (
            "pub is added to another definition",
            "pub fn twice(n: Int) -> Int { helper() * n }\npub fn helper() -> Int { 2 }\n",
        ),
        (
            "an export is added",
            "pub fn twice(n: Int) -> Int { n * 2 }\npub fn thrice(n: Int) -> Int { n * 3 }\n",
        ),
        (
            "an export is renamed",
            "pub fn doubled(n: Int) -> Int { n * 2 }\n",
        ),
    ];

    for (what, b) in edits {
        let dir = tempfile::tempdir().expect("tempdir");
        two_modules(dir.path(), B);
        check(dir.path(), "a.nz");

        write(dir.path(), "b.nz", b);
        let after = check(dir.path(), "a.nz");
        assert_eq!(
            after.reused, PRELUDE,
            "{what}: the importer kept a stale answer"
        );
        assert_eq!(
            meaning(&after),
            meaning(&cold(dir.path(), "a.nz")),
            "{what}"
        );
    }
}

// --- N36: effect contracts cross the boundary, bodies do not ----------------------------

fn pure_importer(dir: &Path, b: &str) {
    project(dir);
    write(
        dir,
        "a.nz",
        "use \"b.nz\";\nfn pure() -> Int ! {} { twice(21) }\nfn main() -> Int { pure() }\n",
    );
    write(dir, "b.nz", b);
}

#[test]
fn an_effect_only_change_rechecks_the_importer_and_finds_what_it_breaks() {
    // The body is the same bytes; only the contract moves. It is in B's interface, so it
    // is in A's key, so A is checked — and A's purity assertion now fails.
    let dir = tempfile::tempdir().expect("tempdir");
    pure_importer(dir.path(), "pub fn twice(n: Int) -> Int ! {} { n * 2 }\n");
    assert!(check(dir.path(), "a.nz").accepted);
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int ! { io } { n * 2 }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (2, PRELUDE));
    assert!(!after.accepted);
    assert!(
        after.diagnostics.iter().all(|d| d.starts_with("N0366")),
        "{after:?}"
    );
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
    // And back: the contract is pure again, and so is the answer.
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int ! {} { n * 2 }\n",
    );
    let back = check(dir.path(), "a.nz");
    assert!(back.accepted, "{back:?}");
}

#[test]
fn a_body_change_within_its_declared_contract_leaves_the_importer_alone() {
    // What another module sees is the contract, so a body that stays inside it reaches
    // nobody: B is checked again, and A is not.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { twice(io, 21) }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(io: IoCap, n: Int) -> Int ! { io } { n * 2 }\n",
    );
    assert!(check(dir.path(), "a.nz").accepted);
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(io: IoCap, n: Int) -> Int ! { io } { print(\"x\") + n * 2 }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert!(after.accepted, "{after:?}");
    assert_eq!((after.checked, after.reused), (1, 1 + PRELUDE));
    // A body that leaves its contract is refused in the module that wrote it.
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(io: IoCap, n: Int) -> Int ! {} { print(\"x\") + n * 2 }\n",
    );
    let broken = check(dir.path(), "a.nz");
    assert!(!broken.accepted);
    assert_eq!(meaning(&broken), meaning(&cold(dir.path(), "a.nz")));
}

#[test]
fn an_import_that_declares_nothing_is_every_effect_to_a_pure_caller() {
    // A caller may not read another module's body, so an undeclared export promises
    // nothing — however pure its body is. Declaring it is the migration. From N37 to N104 it
    // was also every **authority** to a caller with a contract; since N104 a call needs no
    // authority of its caller, so only the effect is refused.
    let dir = tempfile::tempdir().expect("tempdir");
    pure_importer(dir.path(), "pub fn twice(n: Int) -> Int { n * 2 }\n");
    let refused = cold(dir.path(), "a.nz");
    assert!(!refused.accepted);
    let codes: Vec<&str> = refused.diagnostics.iter().map(|d| &d[..5]).collect();
    assert_eq!(codes, ["N0366"], "{refused:?}");
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int ! {} { n * 2 }\n",
    );
    assert!(cold(dir.path(), "a.nz").accepted);
}

#[test]
fn the_interface_a_cache_keeps_carries_the_declared_set_and_not_the_inferred_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int { loud(io) + quiet() + plain(io) }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "pub fn loud(io: IoCap) -> Int ! { io } { print(\"x\") }\npub fn quiet() -> Int ! {} { 1 }\npub fn plain(out: OutCap) -> Int { print(\"y\") }\n",
    );
    assert!(check(dir.path(), "a.nz").accepted);
    let b = check_entries(dir.path())
        .into_iter()
        .map(|(_, a)| a)
        .find(|a| a.module == "b.nz")
        .expect("b's entry");
    let text = b.interface.to_json().expect("json");
    assert!(
        text.contains(
            r#""name":"loud","kind":"fn","generics":0,"params":["IoCap"],"ret":"Int","effects":["io"]"#
        ),
        "{text}"
    );
    assert!(
        text.contains(
            r#""name":"quiet","kind":"fn","generics":0,"params":[],"ret":"Int","effects":[]"#
        ),
        "{text}"
    );
    assert!(
        text.contains(
            r#""name":"plain","kind":"fn","generics":0,"params":["OutCap"],"ret":"Int"}"#
        ),
        "{text}"
    );
}

// --- N37: authority crosses the boundary in signatures ------------------------------------

#[test]
fn a_capability_only_change_rechecks_the_importer_and_finds_what_it_breaks() {
    // The body and the declared set are the same bytes; only the authority `save` requires
    // moves. It is a parameter, so it is in B's interface and in A's key: A is checked, and
    // its call no longer supplies what `save` needs.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { save(io, 1) }\n",
    );
    let body = "! { io } { print(int_to_str(n)) }\n";
    write(
        dir.path(),
        "b.nz",
        &format!("pub fn save(io: IoCap, n: Int) -> Int {body}"),
    );
    assert!(check(dir.path(), "a.nz").accepted);
    write(
        dir.path(),
        "b.nz",
        &format!("pub fn save(io: IoCap, tasks: SpawnCap, n: Int) -> Int {body}"),
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (2, PRELUDE));
    assert!(!after.accepted);
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
    // A different kind in the same position, under the same name, is a different contract
    // too: from the accepted program, only `IoCap` becomes `SpawnCap`.
    write(
        dir.path(),
        "b.nz",
        &format!("pub fn save(io: IoCap, n: Int) -> Int {body}"),
    );
    assert!(check(dir.path(), "a.nz").accepted);
    write(
        dir.path(),
        "b.nz",
        "pub fn save(io: SpawnCap, n: Int) -> Int ! { io } { n }\n",
    );
    let kind = check(dir.path(), "a.nz");
    assert_eq!((kind.checked, kind.reused), (2, PRELUDE), "{kind:?}");
    assert!(
        !kind.diagnostics.is_empty() && kind.diagnostics.iter().all(|d| d.starts_with("N0300")),
        "{kind:?}"
    );
}

#[test]
fn an_undeclared_import_needs_no_authority_of_its_caller() {
    // N104: an export that declares nothing holds what it exercises, as every function does,
    // so its caller needs nothing at the call. What remains of N37's bridge is the effect a
    // caller with a contract must allow: every effect, since the import publishes none.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "b.nz",
        "pub fn old(n: Int, out: OutCap) -> Int { print(int_to_str(n)) }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io, spawn } { old(1, io) }\n",
    );
    assert!(cold(dir.path(), "a.nz").accepted);
    // A contract narrower than every effect is refused for the effect, not for authority.
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { old(1, io) }\n",
    );
    let refused = cold(dir.path(), "a.nz");
    assert_eq!(refused.diagnostics.len(), 1, "{refused:?}");
    assert!(refused.diagnostics[0].starts_with("N0366"), "{refused:?}");
    // And the import itself, given nothing, is refused where it prints.
    write(
        dir.path(),
        "b.nz",
        "pub fn old(n: Int) -> Int { print(int_to_str(n)) }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { old(1) }\n",
    );
    let refused = cold(dir.path(), "a.nz");
    assert!(
        refused.diagnostics.iter().any(|d| d.starts_with("N0369")),
        "{refused:?}"
    );
}

// --- N38: provenance crosses as facts, and is solved every run ------------------------------

#[test]
fn a_callees_provenance_reaches_a_caller_the_cache_did_not_check_again() {
    // B's body changes what `target` returns — a constant, then a file's contents — and
    // nothing B declares moves, so A's key does not either and A is reused. The flow is
    // solved over every module's facts every run, so A is still told, and refused.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { write_file(target(io), \"x\"); 0 }\n",
    );
    let local = "pub fn target(io: IoCap) -> Str ! { io } { \"out.txt\" }\n";
    write(dir.path(), "b.nz", local);
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");
    write(
        dir.path(),
        "b.nz",
        "pub fn target(io: IoCap) -> Str ! { io } { read_file(\"manifest\") }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (1, 1 + PRELUDE), "{after:?}");
    assert!(!after.accepted);
    assert_eq!(after.diagnostics.len(), 1, "{after:?}");
    assert!(after.diagnostics[0].starts_with("N0372"), "{after:?}");
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
    // Once more with every module reused: the facts come from the entries.
    let again = check(dir.path(), "a.nz");
    assert_eq!(again.reused, 2 + PRELUDE, "{again:?}");
    assert_eq!(meaning(&again), meaning(&after));
    // And back: nothing stale either way.
    write(dir.path(), "b.nz", local);
    let back = check(dir.path(), "a.nz");
    assert!(back.accepted, "{back:?}");
}

#[test]
fn a_container_written_in_a_module_the_cache_reused_still_reaches_its_reader() {
    // N80: A reads a `Strs` into a path, and B is the only writer of any `Strs`. B's body
    // decides what the cell holds; nothing B declares moves, so A is reused, and still told.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { let xs = names(io); write_file(strs_get(xs, 0), \"x\") }\n",
    );
    let from_args = "pub fn names(io: IoCap) -> Strs ! { io } { let xs = strs_new(); strs_push(xs, arg(1)); xs }\n";
    write(dir.path(), "b.nz", from_args);
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");
    write(
        dir.path(),
        "b.nz",
        "pub fn names(io: IoCap) -> Strs ! { io } { let xs = strs_new(); strs_push(xs, read_file(\"m\")); xs }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (1, 1 + PRELUDE), "{after:?}");
    assert!(!after.accepted);
    assert!(after.diagnostics[0].starts_with("N0372"), "{after:?}");
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
    // Every module reused: the cells come from the entries' facts.
    let again = check(dir.path(), "a.nz");
    assert_eq!(again.reused, 2 + PRELUDE, "{again:?}");
    assert_eq!(meaning(&again), meaning(&after));
    write(dir.path(), "b.nz", from_args);
    let back = check(dir.path(), "a.nz");
    assert!(back.accepted, "{back:?}");
}

#[test]
fn a_function_value_made_in_a_reused_module_is_still_a_candidate() {
    // N80: B makes the only function value of its type; A calls through one. A changed body
    // in B changes what A's indirect call returns, with A reused.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { let f = maker(); write_file(f(io), \"x\") }\n",
    );
    let local = "pub fn maker() -> fn(IoCap) -> Str ! { io } ! {} { fn(c: IoCap) -> Str ! { io } { \"out\" } }\n";
    write(dir.path(), "b.nz", local);
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");
    write(
        dir.path(),
        "b.nz",
        "pub fn maker() -> fn(IoCap) -> Str ! { io } ! {} { fn(c: IoCap) -> Str ! { io } { read_file(\"m\") } }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (1, 1 + PRELUDE), "{after:?}");
    assert!(!after.accepted, "{after:?}");
    assert!(after.diagnostics[0].starts_with("N0372"), "{after:?}");
    // B makes a closure, so it files no entry and is checked again (Q1-B-C1); A is reused.
    let again = check(dir.path(), "a.nz");
    assert_eq!(again.reused, 1 + PRELUDE, "{again:?}");
    assert_eq!(meaning(&again), meaning(&after));
}

/// Q1-B-C1: a cache hit must not change a verdict. B makes a closure that returns a literal, and
/// A writes to the path it returns: accepted cold, and accepted on every warm run after it. Before
/// the fix the second run refused it (`N0372`): B's entry named a closure a reused module does not
/// have, so the whole program's flow was solved as incomplete.
#[test]
fn a_module_that_makes_a_closure_gives_the_same_verdict_warm_as_cold() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { let f = maker(); write_file(f(io), \"x\") }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "pub fn maker() -> fn(IoCap) -> Str ! { io } ! {} { fn(c: IoCap) -> Str ! { io } { \"out\" } }\n",
    );
    let cold = cold(dir.path(), "a.nz");
    assert!(cold.accepted, "{cold:?}");
    for run in 0..3 {
        let warm = check(dir.path(), "a.nz");
        assert_eq!(meaning(&warm), meaning(&cold), "run {run}: {warm:?}");
    }
}

#[test]
fn a_helper_in_another_module_that_writes_to_its_parameter_carries_the_rule_across() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "b.nz",
        "pub fn save(io: IoCap, p: Str) -> Int ! { io } { write_file(p, \"x\") }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { save(io, read_file(\"cfg\")) }\n",
    );
    let warm = check(dir.path(), "a.nz");
    assert!(
        warm.diagnostics.iter().all(|d| d.starts_with("N0372")) && !warm.accepted,
        "{warm:?}"
    );
    let reused = check(dir.path(), "a.nz");
    assert_eq!(verdict(&reused), verdict(&warm));
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int ! { io } { save(io, arg(0)) }\n",
    );
    assert!(check(dir.path(), "a.nz").accepted);
}

#[test]
fn an_entry_keeps_the_modules_provenance_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main(io: IoCap) -> Int { str_len(load(io)) }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "pub fn load(io: IoCap) -> Str { read_file(\"f\") }\n",
    );
    assert!(check(dir.path(), "a.nz").accepted);
    let b = check_entries(dir.path())
        .into_iter()
        .map(|(_, a)| a)
        .find(|a| a.module == "b.nz")
        .expect("b's entry");
    assert_eq!(b.schema, "nazm.check/5");
    let f = &b.provenance.functions[0];
    assert_eq!(f.name, "load");
    assert_eq!(f.result.origins.len(), 1);
    assert_eq!(f.result.origins[0].0, "file");
}

// --- §35: A → B → C, and where propagation stops -----------------------------------------

fn three_modules(dir: &Path, c: &str) {
    project(dir);
    write(dir, "a.nz", "use \"b.nz\";\nfn main() -> Int { mid(1) }\n");
    write(
        dir,
        "b.nz",
        "use \"c.nz\";\npub fn mid(n: Int) -> Int { deep(n) }\n",
    );
    write(dir, "c.nz", c);
}

const C: &str = "pub fn deep(n: Int) -> Int { inner() + n }\nfn inner() -> Int { 7 }\n";

#[test]
fn a_private_change_three_modules_down_reaches_nobody() {
    // Case 1. C is checked because its own source moved; B and A are not, because what C
    // offers did not.
    let dir = tempfile::tempdir().expect("tempdir");
    three_modules(dir.path(), C);
    assert_eq!(check(dir.path(), "a.nz").checked, 3 + PRELUDE);

    write(
        dir.path(),
        "c.nz",
        "pub fn deep(n: Int) -> Int { inner() + n }\nfn inner() -> Int { 3 + 4 }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (1, 2 + PRELUDE));
    assert!(after.accepted);
}

#[test]
fn an_exported_change_propagates_one_step_and_stops() {
    // Case 2, and the one the whole design is for. C's export changes, so B must be
    // checked again — but B's own interface is unchanged by that, so **A is not**.
    // Invalidation follows interfaces, not reachability: a global sweep would have
    // re-checked A here, and would be wrong about nothing except how much work it did.
    let dir = tempfile::tempdir().expect("tempdir");
    three_modules(dir.path(), C);
    assert_eq!(check(dir.path(), "a.nz").checked, 3 + PRELUDE);

    // `deep` gains a parameter. B's body changes to match — so B is re-checked for two
    // reasons — and B's signature does not, which is the point.
    write(
        dir.path(),
        "c.nz",
        "pub fn deep(n: Int, k: Int) -> Int { inner() + n + k }\nfn inner() -> Int { 7 }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "use \"c.nz\";\npub fn mid(n: Int) -> Int { deep(n, 0) }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        (after.checked, after.reused),
        (2, 1 + PRELUDE),
        "A's interface dependency on B did not move, so A should have been left alone"
    );
    assert!(after.accepted);
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
}

#[test]
fn a_change_that_moves_every_interface_reaches_every_importer() {
    // Case 3. C's export changes and B's does too, so all three are checked. The
    // difference from case 2 is one line of B, which is exactly the difference the model
    // says should matter.
    let dir = tempfile::tempdir().expect("tempdir");
    three_modules(dir.path(), C);
    assert_eq!(check(dir.path(), "a.nz").checked, 3 + PRELUDE);

    write(
        dir.path(),
        "c.nz",
        "pub fn deep(n: Int, k: Int) -> Int { inner() + n + k }\nfn inner() -> Int { 7 }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "use \"c.nz\";\npub fn mid(n: Int, k: Int) -> Int { deep(n, k) }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { mid(1, 2) }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (3, PRELUDE));
    assert!(after.accepted);
}

// --- §36: a cache may change the work, not the meaning ------------------------------------

#[test]
fn cold_warm_and_disabled_agree_on_every_program() {
    // Accepted and refused alike, and for the refused ones the code **and the position**
    // have to match: a cache that dropped a diagnostic, or moved one, would be reporting
    // about a compilation that did not happen.
    let programs: &[(&str, &str, &str)] = &[
        (
            "accepted",
            "use \"b.nz\";\nfn main() -> Int { twice(1) }\n",
            B,
        ),
        (
            "a type error in the importer",
            "use \"b.nz\";\nfn main() -> Int { twice(\"x\") }\n",
            B,
        ),
        (
            "a private definition named across a module boundary",
            "use \"b.nz\";\nfn main() -> Int { helper() }\n",
            B,
        ),
        (
            "an undefined name",
            "use \"b.nz\";\nfn main() -> Int { nowhere() }\n",
            B,
        ),
        (
            "an import that collides with a definition",
            "use \"b.nz\";\nfn twice(n: Int) -> Int { n }\nfn main() -> Int { twice(1) }\n",
            B,
        ),
        (
            "a program with no main",
            "use \"b.nz\";\npub fn other() -> Int { twice(1) }\n",
            B,
        ),
        (
            "an error in the dependency",
            "use \"b.nz\";\nfn main() -> Int { twice(1) }\n",
            "pub fn twice(n: Int) -> Int { missing() }\n",
        ),
    ];

    for (what, a, b) in programs {
        let dir = tempfile::tempdir().expect("tempdir");
        project(dir.path());
        write(dir.path(), "a.nz", a);
        write(dir.path(), "b.nz", b);

        let disabled = cold(dir.path(), "a.nz");
        let first = check(dir.path(), "a.nz");
        let warm = check(dir.path(), "a.nz");
        assert_eq!(meaning(&disabled), meaning(&first), "{what}: cold");
        assert_eq!(meaning(&disabled), meaning(&warm), "{what}: warm");
        assert!(!disabled.diagnostics.is_empty() || disabled.accepted);
    }
}

#[test]
fn a_program_that_does_not_check_is_never_reused() {
    // An entry means *this checked cleanly*. A refusal is recomputed from source every
    // run, so it cannot be cached, cannot go stale, and cannot be suppressed.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(dir.path(), "a.nz", "fn main() -> Int { nowhere() }\n");

    let first = check(dir.path(), "a.nz");
    assert!(!first.accepted);
    // The prelude checked cleanly, and an entry means *this module* did.
    assert_eq!(first.stored, PRELUDE);

    let second = check(dir.path(), "a.nz");
    assert_eq!(second.reused, PRELUDE, "a refusal must not be reused");
    assert_eq!(meaning(&first), meaning(&second));
}

#[test]
fn deleting_the_cache_changes_nothing() {
    // §14. It is derived data: removing it costs work and decides nothing.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);
    let warm_setup = check(dir.path(), "a.nz");
    assert_eq!(warm_setup.stored, 2 + PRELUDE);

    std::fs::remove_dir_all(dir.path().join(".nazm")).expect("remove the cache");
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (2 + PRELUDE, 0));
    assert_eq!(meaning(&after), meaning(&warm_setup));
}

#[test]
fn a_disabled_cache_writes_nothing_and_reads_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);

    let disabled = cold(dir.path(), "a.nz");
    assert_eq!((disabled.reused, disabled.stored), (0, 0));
    assert!(
        !dir.path().join(".nazm").exists(),
        "nothing should be written"
    );

    // And a warm store is ignored rather than consulted.
    check(dir.path(), "a.nz");
    assert_eq!(cold(dir.path(), "a.nz").reused, 0);
}

// --- §37: nothing on disk is authority ----------------------------------------------------

/// Every entry the store holds.
fn entries(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let root = dir.join(".nazm/check");
    for bucket in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        for entry in std::fs::read_dir(bucket.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if entry.path().extension().is_some_and(|x| x == "json") {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out
}

/// The entry for the module whose durable key is `key`.
/// The entry **for** that module, not one that merely mentions it.
///
/// `"module"` appears twice in an entry's bytes: once as the module the entry is about,
/// and once per dependency. Matching the bare field found whichever file the directory
/// happened to yield first, which is a digest and therefore arbitrary — and it changed
/// when the interface schema did, so a test that had been passing started damaging an
/// importer's entry instead of the one it named. The field that follows tells them apart:
/// an entry's own module is followed by its source fingerprint, a dependency's by the
/// interface hash it was checked against.
fn entry_for(dir: &Path, key: &str) -> std::path::PathBuf {
    let wanted = format!(r#""module":"{key}","source""#);
    entries(dir)
        .into_iter()
        .find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains(&wanted)))
        .expect("an entry for that module")
}

/// Replace the 64-character digest after `"<field>":"`.
fn swap_digest(text: &str, field: &str) -> String {
    let marker = format!(r#""{field}":""#);
    let at = text.find(&marker).expect("the field") + marker.len();
    format!("{}{}{}", &text[..at], "0".repeat(64), &text[at + 64..])
}

#[test]
fn a_damaged_entry_is_recomputed_rather_than_believed() {
    // One entry is damaged — B's — and exactly one module is therefore checked again. The
    // second half of each case is the interesting one: A keeps its entry, because A's key
    // depends on what B *offers*, which stage 1 recomputes from source and never reads
    // from a file.
    /// One way of damaging an entry: what it is called, and what it does to the bytes.
    type Damage = (&'static str, fn(&str) -> String);

    let damage: &[Damage] = &[
        ("truncated", |t| t[..t.len() / 2].to_owned()),
        ("emptied", |_| String::new()),
        ("not json at all", |_| "nonsense".to_owned()),
        ("a schema from the future", |t| {
            t.replace("nazm.check/5", "nazm.check/9")
        }),
        ("an edited interface", |t| {
            t.replace(r#""ret":"Int""#, r#""ret":"Str""#)
        }),
        ("an edited source fingerprint", |t| swap_digest(t, "source")),
        ("a foreign checker", |t| swap_digest(t, "checker")),
        ("an edited module key", |t| {
            t.replacen(r#""module":"b.nz""#, r#""module":"z.nz""#, 1)
        }),
        // The provenance facts and resolved unit are sealed by a digest (Q1-M-07).
        ("an edited facts digest", |t| {
            t.replacen(
                r#""facts_digest":"blake3:"#,
                r#""facts_digest":"blake3:f"#,
                1,
            )
        }),
    ];

    for (what, break_it) in damage {
        let dir = tempfile::tempdir().expect("tempdir");
        two_modules(dir.path(), B);
        let good = check(dir.path(), "a.nz");
        assert!(good.accepted);

        let path = entry_for(dir.path(), "b.nz");
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::write(&path, break_it(&text)).expect("damage it");

        let after = check(dir.path(), "a.nz");
        assert!(after.accepted, "{what}: a damaged entry changed the answer");
        assert_eq!(meaning(&after), meaning(&good), "{what}");
        assert_eq!(
            (after.checked, after.reused, after.rejected),
            (1, 1 + PRELUDE, 1),
            "{what}: a damaged entry must be a miss, and only for its own module"
        );
    }
}

#[test]
fn a_damaged_entry_cannot_hide_an_error() {
    // The dangerous direction, and the one a test that only checked accepted programs
    // would miss: the program is wrong, and no arrangement of bytes on disk may make the
    // compiler say otherwise.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);
    check(dir.path(), "a.nz");
    let stored: Vec<(std::path::PathBuf, String)> = entries(dir.path())
        .into_iter()
        .map(|p| {
            let t = std::fs::read_to_string(&p).expect("read");
            (p, t)
        })
        .collect();

    // A now calls `twice` with the wrong type. Its own entry is stale by construction —
    // and here it is restored on top of the new source, which is the strongest form of
    // the attack.
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { twice(\"x\") }\n",
    );
    for (path, text) in &stored {
        std::fs::write(path, text).expect("restore");
    }

    let after = check(dir.path(), "a.nz");
    assert!(!after.accepted, "a stale entry accepted a wrong program");
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
}

/// Q1-M-07: an entry's provenance facts are not re-derived on read, so they are sealed by a
/// digest. Emptying a module's sink list in its entry must not let a caller's new flow into that
/// sink be accepted: the edited entry is refused and the module checked again.
#[test]
fn an_edited_provenance_fact_cannot_hide_a_flow() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "store.nz",
        "pub fn save(io: IoCap, path: Str, text: Str) -> Int ! { io } {\n    write_file(path, text)\n}\n",
    );
    write(
        dir.path(),
        "main.nz",
        "use \"store.nz\";\nfn main(io: IoCap) -> Int ! { io } {\n    save(io, \"out.txt\", \"hi\")\n}\n",
    );
    assert!(check(dir.path(), "main.nz").accepted);

    let path = entry_for(dir.path(), "store.nz");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    let mut emptied = 0;
    for f in v["provenance"]["functions"]
        .as_array_mut()
        .expect("functions")
    {
        if let Some(sinks) = f.get_mut("sinks") {
            *sinks = serde_json::Value::Array(Vec::new());
            emptied += 1;
        }
    }
    assert!(emptied > 0, "the entry records `save`'s sink");
    std::fs::write(&path, v.to_string()).expect("edit it");

    write(
        dir.path(),
        "main.nz",
        "use \"store.nz\";\nfn main(io: IoCap) -> Int ! { io } {\n    save(io, read_file(\"cfg\"), \"hi\")\n}\n",
    );
    let warm = check(dir.path(), "main.nz");
    let cold = cold(dir.path(), "main.nz");
    assert!(!cold.accepted, "the flow is refused from source");
    assert!(!warm.accepted, "an edited entry accepted a refused flow");
    assert!(
        warm.rejected >= 1,
        "the edited entry is refused, not believed"
    );
    assert_eq!(meaning(&warm), meaning(&cold));
}

// --- §38, §39: identity across checkouts and across entry points --------------------------

#[test]
fn two_checkouts_of_one_project_share_entries() {
    // Nothing about where a checkout sits reaches a key, so the work done in one is
    // reused in the other. They share a store here only because they must be made to:
    // the default store lives beside the project, which is the point of it.
    let shared = tempfile::tempdir().expect("tempdir");
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    assert_ne!(a.path(), b.path());
    two_modules(a.path(), B);
    two_modules(b.path(), B);

    let with_shared = |dir: &Path| {
        let out = Command::new(NAZM)
            .args(["check", "a.nz", "--cache-report"])
            .env("NAZM_CACHE_DIR", shared.path())
            .current_dir(dir)
            .output()
            .expect("run");
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text
            .lines()
            .find(|l| l.contains("nazm.check-report/1"))
            .expect("a report")
            .to_owned();
        let v: serde_json::Value = serde_json::from_str(&line).expect("a report");
        (
            out.status.success(),
            v["checked"].as_u64().unwrap_or(9),
            v["reused"].as_u64().unwrap_or(9),
        )
    };

    assert_eq!(with_shared(a.path()), (true, 2 + PRELUDE, 0));
    assert_eq!(
        with_shared(b.path()),
        (true, 0, 2 + PRELUDE),
        "a different directory should not be a different project"
    );
}

/// Two applications at different depths over one library.
///
/// The depth is the point: it is what makes the derived root differ between the two
/// compilations, which is the N3 limitation a declared root removes.
fn two_entry_points(dir: &Path) {
    write(
        dir,
        "app.nz",
        "use \"lib/common.nz\";\nfn main() -> Int { shared(1) }\n",
    );
    write(
        dir,
        "tool/run.nz",
        "use \"../lib/common.nz\";\nfn main() -> Int { shared(2) }\n",
    );
    write(
        dir,
        "lib/common.nz",
        "pub fn shared(n: Int) -> Int { helper() + n }\nfn helper() -> Int { 3 }\n",
    );
}

#[test]
fn a_shared_module_is_reused_across_two_entry_points() {
    // §39, and the reason N4 needed a declared root at all. `lib/common.nz` is the same
    // module whichever application names it, so checking the second one finds the first
    // one's work — even though the two compilations begin in different directories.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    two_entry_points(dir.path());

    let first = check(dir.path(), "app.nz");
    assert_eq!((first.checked, first.reused), (2 + PRELUDE, 0));

    let second = check(dir.path(), "tool/run.nz");
    assert_eq!(
        (second.checked, second.reused),
        (1, 1 + PRELUDE),
        "the library was checked for `app.nz`; `tool/run.nz` should not check it again"
    );
    assert!(second.accepted);
}

#[test]
fn without_a_declared_root_the_two_entry_points_share_nothing() {
    // The same tree with no marker, and the limitation made visible rather than argued.
    // `tool/run.nz`'s compilation measures from `tool/`, so the library is *above* its
    // root and has no durable identity there at all. Both compilations are correct;
    // neither can use the other's work.
    let dir = tempfile::tempdir().expect("tempdir");
    two_entry_points(dir.path());

    let first = check(dir.path(), "app.nz");
    assert_eq!(first.root, "derived");
    assert_eq!((first.checked, first.reused), (2 + PRELUDE, 0));

    // Each derived root has a store of its own, so not even the prelude — whose key is
    // the same in both — is shared.
    let second = check(dir.path(), "tool/run.nz");
    assert_eq!((second.checked, second.reused), (2 + PRELUDE, 0));
    assert!(second.declined >= 1, "{second:?}");
    assert!(second.accepted);
}

#[test]
fn two_modules_with_identical_source_do_not_share_an_entry() {
    // Which module this is, is an input. Two modules can have byte-identical source and
    // the same (empty) dependency set; if the key did not say which module it was for,
    // the first one's result would be handed to the second. They are different modules
    // with different durable identities, and one's checked interface is not the other's.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    let same = "pub fn v() -> Int { 1 }\n";
    write(dir.path(), "a/m.nz", same);
    write(dir.path(), "b/m.nz", same);
    write(
        dir.path(),
        "one.nz",
        "use \"a/m.nz\";\nfn main() -> Int { v() }\n",
    );
    write(
        dir.path(),
        "two.nz",
        "use \"b/m.nz\";\nfn main() -> Int { v() }\n",
    );

    let first = check(dir.path(), "one.nz");
    assert_eq!((first.checked, first.stored), (2 + PRELUDE, 2 + PRELUDE));

    let second = check(dir.path(), "two.nz");
    assert_eq!(
        (second.checked, second.reused, second.rejected),
        (2, PRELUDE, 0),
        "`b/m.nz` has the same bytes as `a/m.nz` and is not the same module"
    );
    assert!(second.accepted);
}

// --- §40: where durable identity declines -------------------------------------------------

#[test]
fn a_module_with_no_durable_identity_is_checked_every_time() {
    // Not refused, not guessed at: checked. An optimisation's limit may not become a
    // language rejection.
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "outside.nz", "pub fn v() -> Int { 4 }\n");
    write(dir.path(), "project/nazm.root", "");
    write(
        dir.path(),
        "project/main.nz",
        "use \"../outside.nz\";\nfn main() -> Int { v() }\n",
    );

    let first = check(dir.path(), "project/main.nz");
    assert!(first.accepted);
    assert!(first.declined >= 1, "{first:?}");

    // And it stays that way: the module above the root never acquires a key, so it is
    // never reused, run after run.
    let second = check(dir.path(), "project/main.nz");
    assert!(second.accepted);
    assert_eq!(
        second.reused, PRELUDE,
        "nothing here but the prelude should have a key: {second:?}"
    );
}

// --- §19: two compilers at once -----------------------------------------------------------

#[test]
fn two_compilers_publishing_at_once_leave_a_usable_store() {
    // Entries are addressed by everything they depend on, so two processes computing the
    // same result compute the same bytes. Whichever rename lands second lands the same
    // file, which is why this needs no lock.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);

    let racers: Vec<_> = (0..4)
        .map(|_| {
            Command::new(NAZM)
                .args(["check", "a.nz"])
                .current_dir(dir.path())
                .spawn()
                .expect("spawn")
        })
        .collect();
    for mut r in racers {
        let status = r.wait().expect("wait");
        assert!(status.success(), "a concurrent check failed");
    }

    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (0, 2 + PRELUDE));
    assert!(after.accepted);
}

// --- records, and which edits reach an importer -----------------------------------------

/// A → B where B exports a record and a function over it.
fn two_modules_with_a_record(dir: &Path, b: &str) {
    project(dir);
    write(
        dir,
        "a.nz",
        "use \"b.nz\";\n\
         fn size(p: Point) -> Int { p.x }\n\
         fn main() -> Int { size(at(21)) + Point(x: 21).x }\n",
    );
    write(dir, "b.nz", b);
}

const RECORD_B: &str = "pub struct Point {\n    x: Int,\n}\n\
                        struct Hidden {\n    n: Int,\n}\n\
                        fn secret() -> Int { Hidden(n: 1).n }\n\
                        pub fn at(n: Int) -> Point { Point(x: n + secret() - 1) }\n";

#[test]
fn a_private_record_change_leaves_the_importer_alone() {
    // The record analogue of the decisive case. B's private record changes shape: B's
    // source fingerprint moves, so B is checked again; B's *interface* does not, so A is
    // not. A record is not special here — it goes through the same fingerprint.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules_with_a_record(dir.path(), RECORD_B);
    let first = check(dir.path(), "a.nz");
    assert_eq!((first.checked, first.reused), (2 + PRELUDE, 0));

    write(
        dir.path(),
        "b.nz",
        "pub struct Point {\n    x: Int,\n}\n\
         struct Hidden {\n    n: Str,\n    extra: Int,\n}\n\
         fn secret() -> Int { str_len(Hidden(n: \"\", extra: 0).n) + 1 }\n\
         pub fn at(n: Int) -> Point { Point(x: n + secret() - 1) }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        (after.checked, after.reused),
        (1, 1 + PRELUDE),
        "exactly B should have been checked"
    );
    assert!(after.accepted);
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
}

#[test]
fn an_exported_record_change_rechecks_the_importer() {
    // The other half: each of these moves B's interface, so A's key moves with it.
    let edits: &[(&str, &str)] = &[
        (
            "a field is added",
            "pub struct Point {\n    x: Int,\n    y: Int,\n}\n\
             pub fn at(n: Int) -> Point { Point(x: n, y: 0) }\n",
        ),
        (
            "a field changes type",
            "pub struct Point {\n    x: Str,\n}\n\
             pub fn at(n: Int) -> Point { Point(x: int_to_str(n)) }\n",
        ),
        (
            "a record is added",
            "pub struct Point {\n    x: Int,\n}\n\
             pub struct Other {\n    y: Int,\n}\n\
             pub fn at(n: Int) -> Point { Point(x: n) }\n",
        ),
    ];
    for (what, after_text) in edits {
        let dir = tempfile::tempdir().expect("tempdir");
        two_modules_with_a_record(dir.path(), RECORD_B);
        check(dir.path(), "a.nz");
        write(dir.path(), "b.nz", after_text);
        let after = check(dir.path(), "a.nz");
        assert_eq!(
            after.reused, PRELUDE,
            "{what}: A should have been re-checked"
        );
    }
}

#[test]
fn reordering_an_exported_records_fields_reaches_nobody() {
    // The claim `docs/spec.md` makes about field order, measured where it is easiest to
    // get wrong: B's source moves, so B re-checks, and A does not — because the
    // interface's fields are sorted by name and the bytes did not move.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules_with_a_record(
        dir.path(),
        "pub struct Point {\n    x: Int,\n    y: Int,\n}\n\
         pub fn at(n: Int) -> Point { Point(x: n, y: 0) }\n",
    );
    // This B's `Point` has two fields, so A's own construction names both.
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n\
         fn size(p: Point) -> Int { p.x }\n\
         fn main() -> Int { size(at(21)) + Point(x: 21, y: 0).x }\n",
    );
    let first = check(dir.path(), "a.nz");
    assert_eq!(
        (first.checked, first.stored),
        (2 + PRELUDE, 2 + PRELUDE),
        "both should be stored"
    );
    write(
        dir.path(),
        "b.nz",
        "pub struct Point {\n    y: Int,\n    x: Int,\n}\n\
         pub fn at(n: Int) -> Point { Point(x: n, y: 0) }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        (after.checked, after.reused),
        (1, 1 + PRELUDE),
        "reordering fields should reach nobody"
    );
}

#[test]
fn an_entry_written_before_records_or_enums_existed_cannot_become_a_hit() {
    // §81. A cached answer is keyed by the checker that produced it, and that identity
    // reads the interface schema and the semantic epoch — both of which moved in N9 and
    // again in N10. So an entry from a build that had no concept of a record, or of an
    // enum, is unreachable by construction rather than by a rule someone has to remember,
    // and nothing deletes it.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules_with_a_record(dir.path(), RECORD_B);
    let first = check(dir.path(), "a.nz");
    assert_eq!(first.stored, 2 + PRELUDE);

    // The identity is a digest of the build, so forging an older one is not possible from
    // out here. What *is* checkable is that the identity moved: an entry stored under the
    // previous epoch is not this build's, and this build's key is what the store is asked
    // for. The epoch is asserted directly.
    assert_eq!(
        nazm_cache::SEMANTIC_EPOCH,
        37,
        "N9, N10, N11, N12, N13, N14, N36, N37, N38, N42, N49, N50, N51, N52, N53, N54, N55, \
         N62, N66, N76, N78, N79, N80, N85, N86, N87, N103, N104, Gate 1-C1, Gate 2 (three \
         times), M1 and Gate 2 three times more each changed what the checker accepts, so the \
         epoch moved thirty-six times"
    );
    assert_eq!(nazm_iface::SCHEMA, "nazm.interface/11");
}

/// Every check entry under `dir`'s derived store, with the directory its store is at.
fn check_entries(dir: &Path) -> Vec<(std::path::PathBuf, nazm_cache::CheckArtifact)> {
    let mut out = Vec::new();
    let mut work = vec![dir.join(nazm_cache::store::DIRECTORY)];
    while let Some(d) = work.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in read.flatten() {
            let path = e.path();
            if path.is_dir() {
                work.push(path);
            } else if path.extension().is_some_and(|x| x == "json") {
                let text = std::fs::read_to_string(&path).expect("read");
                if let Ok(a) = serde_json::from_str::<nazm_cache::CheckArtifact>(&text) {
                    if a.schema == nazm_cache::SCHEMA {
                        let store = path.parent().and_then(Path::parent).expect("a store");
                        out.push((store.to_path_buf(), a));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn an_entry_from_the_checker_before_derived_equality_is_never_read() {
    // N13. `==` on a record moved from refused to accepted under the existing `N0304`, so
    // the code set did not move and only the epoch says the checker changed. What an
    // N12 build left in a store — here, each entry this build wrote, re-filed exactly as
    // the same build at epoch 5 would have filed it — must be a miss, and the module
    // checked again, with nothing deleted by hand.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "struct P { x: Int, name: Str, }
         fn main() -> Int { if P(x: 1, name: \"a\") == P(x: 1, name: \"a\") { 1 } else { 0 } }\n",
    );
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");
    assert_eq!(first.stored, 1 + PRELUDE, "{first:?}");

    let current = nazm_cache::CheckerIdentity::current();
    let older = nazm_cache::CheckerIdentity::at_epoch(nazm_cache::SEMANTIC_EPOCH - 1);
    let entries = check_entries(dir.path());
    assert_eq!(entries.len(), 2, "{entries:?}");
    for (store_dir, entry) in &entries {
        assert_eq!(entry.checker, current.to_string());
        let module = nazm_core::sema::key::ModuleKey::parse(&entry.module).expect("module");
        let source = nazm_cache::SourceFingerprint::parse(&entry.source).expect("source");
        let deps: Vec<nazm_cache::Dependency> = entry
            .dependencies
            .iter()
            .map(|d| nazm_cache::Dependency {
                module: nazm_core::sema::key::ModuleKey::parse(&d.module).expect("dep"),
                interface: nazm_iface::InterfaceHash::parse(&d.interface).expect("hash"),
            })
            .collect();
        let forged =
            nazm_cache::CheckArtifact::of(older, &module, source, &deps, entry.interface.clone())
                .expect("an entry");
        assert_ne!(forged.key, entry.key, "the epoch must reach the key");
        let store = nazm_cache::Store::at(store_dir);
        store.put(&forged).expect("publish");
        let name = &entry.key;
        std::fs::remove_file(store_dir.join(&name[..2]).join(format!("{name}.json")))
            .expect("remove this build's entry");
    }

    let again = check(dir.path(), "a.nz");
    assert!(again.accepted, "{again:?}");
    assert_eq!(meaning(&again), meaning(&first));
    assert_eq!(again.reused, 0, "an epoch-5 entry was read: {again:?}");
    assert_eq!(again.checked, 1 + PRELUDE, "{again:?}");
    assert_eq!(again.stored, 1 + PRELUDE, "{again:?}");
}

#[test]
fn a_module_that_uses_an_enum_was_refused_by_the_build_before_this_one() {
    // The N10 half of the epoch argument, and the reason the counter exists rather than
    // being derived from the code set. `enum` and `match` were **reserved words** in N9:
    // a module using either was refused with `N0102`, so an N9 entry about it — if one
    // could be reached — would say the module has a diagnostic where this build says it
    // has none. Nothing in the derived checker-code set records that a refusal went away.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.nz",
        "enum State { Ready, }
fn main() -> Int { match State.Ready() { State.Ready() => 0, } }
",
    );
    let first = check(dir.path(), "a.nz");
    assert_eq!(first.stored, 1 + PRELUDE, "{first:?}");
    let again = check(dir.path(), "a.nz");
    assert_eq!(again.reused, 1 + PRELUDE, "{again:?}");
}

// --- enums, and which edits reach an importer -------------------------------------------

/// A → B where B exports an enum and a function over it, and A matches on it.
fn two_modules_with_an_enum(dir: &Path, b: &str) {
    project(dir);
    write(
        dir,
        "a.nz",
        "use \"b.nz\";\n\
         fn weight(s: State) -> Int { match s { State.Ready() => 21, \
         State.Done(code: c) => c, } }\n\
         fn main() -> Int { weight(start()) + weight(State.Done(code: 21)) }\n",
    );
    write(dir, "b.nz", b);
}

const ENUM_B: &str = "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
                      enum Hidden {\n    A,\n}\n\
                      fn secret() -> Int { match Hidden.A() { Hidden.A() => 0, } }\n\
                      pub fn start() -> State { State.Ready() }\n";

#[test]
fn a_private_enum_change_leaves_the_importer_alone() {
    // B's private enum changes shape: B's source fingerprint moves, so B is checked
    // again; B's *interface* does not, so A is not.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules_with_an_enum(dir.path(), ENUM_B);
    let first = check(dir.path(), "a.nz");
    assert_eq!((first.checked, first.reused), (2 + PRELUDE, 0));

    write(
        dir.path(),
        "b.nz",
        "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
         enum Hidden {\n    A(n: Str),\n    B,\n}\n\
         fn secret() -> Int { match Hidden.B() { Hidden.A(n: s) => str_len(s), \
         Hidden.B() => 0, } }\n\
         pub fn start() -> State { State.Ready() }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        (after.checked, after.reused),
        (1, 1 + PRELUDE),
        "exactly B should have been checked"
    );
    assert!(after.accepted);
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));
}

#[test]
fn a_public_variant_addition_rechecks_the_importer_and_breaks_its_match() {
    // §44, end to end and through the real binary. B gains a variant; B's interface moves;
    // A's semantic key moves with it; A is re-checked and its `match` is no longer
    // exhaustive. This is the cost of a closed enum with no wildcard arm, and it is the
    // intended one.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules_with_an_enum(dir.path(), ENUM_B);
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");

    write(
        dir.path(),
        "b.nz",
        "pub enum State {\n    Ready,\n    Done(code: Int),\n    Failed,\n}\n\
         pub fn start() -> State { State.Ready() }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!(
        after.reused, PRELUDE,
        "A should have been re-checked: {after:?}"
    );
    assert!(!after.accepted, "A's match should no longer be exhaustive");
    assert!(
        after.diagnostics.iter().any(|d| d.starts_with("N0343")),
        "expected a non-exhaustive match: {:?}",
        after.diagnostics
    );
}

#[test]
fn an_exported_enum_change_rechecks_the_importer() {
    // The other edits that move B's interface, so A's key moves with each.
    let edits: &[(&str, &str)] = &[
        (
            "a variant is renamed",
            "pub enum State {\n    Waiting,\n    Done(code: Int),\n}\n\
             pub fn start() -> State { State.Waiting() }\n",
        ),
        (
            "a payload field is added",
            "pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "a payload type changes",
            "pub enum State {\n    Ready,\n    Done(code: Str),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "an enum is added",
            "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
             pub enum Other {\n    A,\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
    ];
    for (what, after_text) in edits {
        let dir = tempfile::tempdir().expect("tempdir");
        two_modules_with_an_enum(dir.path(), ENUM_B);
        check(dir.path(), "a.nz");
        write(dir.path(), "b.nz", after_text);
        let after = check(dir.path(), "a.nz");
        assert_eq!(
            after.reused, PRELUDE,
            "{what}: A should have been re-checked"
        );
    }
}

#[test]
fn reordering_an_exported_enums_variants_reaches_nobody() {
    // §110's high-value row. B's source moves, so B re-checks, and A does not — because
    // the interface's variants are sorted by name and the bytes did not move. The same
    // for the payload fields inside one.
    for (what, edited) in [
        (
            "variants reordered",
            "pub enum State {\n    Done(code: Int, why: Str),\n    Ready,\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
        (
            "payload fields reordered",
            "pub enum State {\n    Ready,\n    Done(why: Str, code: Int),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        project(dir.path());
        write(
            dir.path(),
            "a.nz",
            "use \"b.nz\";\n\
             fn weight(s: State) -> Int { match s { State.Ready() => 21, \
             State.Done(code: c, why: w) => c + str_len(w), } }\n\
             fn main() -> Int { weight(start()) + weight(State.Done(code: 21, why: \"\")) }\n",
        );
        write(
            dir.path(),
            "b.nz",
            "pub enum State {\n    Ready,\n    Done(code: Int, why: Str),\n}\n\
             pub fn start() -> State { State.Ready() }\n",
        );
        let first = check(dir.path(), "a.nz");
        assert_eq!(
            (first.checked, first.stored),
            (2 + PRELUDE, 2 + PRELUDE),
            "{what}: both should be stored"
        );
        write(dir.path(), "b.nz", edited);
        let after = check(dir.path(), "a.nz");
        assert_eq!(
            (after.checked, after.reused),
            (1, 1 + PRELUDE),
            "{what} should reach nobody"
        );
    }
}

// --- N14: a requirement is part of what a module offers -----------------------------------

#[test]
fn a_public_requirement_added_rechecks_and_refuses_the_importer_that_breaks_it() {
    // B's `keep` gains `T: Equality`. B's interface moves, so A's key moves and A is
    // checked again — and refused, because it passes a `Vec[Int]`. Nothing but the
    // fingerprint carried the change.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { vec_len(keep(vec_new[Int]())) }\n",
    );
    write(dir.path(), "b.nz", "pub fn keep[T](x: T) -> T { x }\n");
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");

    write(
        dir.path(),
        "b.nz",
        "pub fn keep[T: Equality](x: T) -> T { x }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert!(!after.accepted, "{after:?}");
    assert_eq!(after.diagnostics.len(), 1, "{after:?}");
    assert!(after.diagnostics[0].starts_with("N0364"), "{after:?}");
    assert_eq!((after.checked, after.reused), (2, PRELUDE), "{after:?}");
    assert_eq!(meaning(&after), meaning(&cold(dir.path(), "a.nz")));

    // And removed again, the importer is accepted again.
    write(dir.path(), "b.nz", "pub fn keep[T](x: T) -> T { x }\n");
    let back = check(dir.path(), "a.nz");
    assert!(back.accepted, "{back:?}");
}

#[test]
fn a_private_requirement_changed_leaves_the_importer_alone() {
    // A requirement on a private helper is B's own business: B's source moves and its
    // interface does not, so B alone is checked again.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { if same_int(1, 1) { 1 } else { 0 } }\n",
    );
    write(
        dir.path(),
        "b.nz",
        "pub fn same_int(a: Int, b: Int) -> Bool { eq(a, b) }\n\
         fn eq[T: Equality](a: T, b: T) -> Bool { a == b }\n",
    );
    check(dir.path(), "a.nz");
    write(
        dir.path(),
        "b.nz",
        "pub fn same_int(a: Int, b: Int) -> Bool { eq(a, b) }\n\
         fn eq[T](a: T, b: T) -> Bool { true }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert!(after.accepted, "{after:?}");
    assert_eq!(
        (after.checked, after.reused, after.missed),
        (1, 1 + PRELUDE, 1),
        "exactly B should have been checked"
    );
}

#[test]
fn an_entry_from_the_checker_before_requirements_is_never_read() {
    // N14. `==` on a parameter that requires `Equality` moved from refused to accepted
    // under the existing `N0304`. An entry the epoch-6 checker filed is a miss.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path());
    write(
        dir.path(),
        "a.nz",
        "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
         fn main() -> Int { if same(1, 1) { 1 } else { 0 } }\n",
    );
    let first = check(dir.path(), "a.nz");
    assert!(first.accepted, "{first:?}");
    let older = nazm_cache::CheckerIdentity::at_epoch(nazm_cache::SEMANTIC_EPOCH - 1);
    for (store_dir, entry) in &check_entries(dir.path()) {
        let module = nazm_core::sema::key::ModuleKey::parse(&entry.module).expect("module");
        let source = nazm_cache::SourceFingerprint::parse(&entry.source).expect("source");
        let deps: Vec<nazm_cache::Dependency> = entry
            .dependencies
            .iter()
            .map(|d| nazm_cache::Dependency {
                module: nazm_core::sema::key::ModuleKey::parse(&d.module).expect("dep"),
                interface: nazm_iface::InterfaceHash::parse(&d.interface).expect("hash"),
            })
            .collect();
        let forged =
            nazm_cache::CheckArtifact::of(older, &module, source, &deps, entry.interface.clone())
                .expect("an entry");
        nazm_cache::Store::at(store_dir)
            .put(&forged)
            .expect("publish");
        let name = &entry.key;
        std::fs::remove_file(store_dir.join(&name[..2]).join(format!("{name}.json")))
            .expect("remove this build's entry");
    }
    let again = check(dir.path(), "a.nz");
    assert!(again.accepted, "{again:?}");
    assert_eq!(again.reused, 0, "an epoch-6 entry was read: {again:?}");
    // N36 moved it again, for effects: `an_entry_from_the_checker_before_effects_is_never_read`,
    // and N37 for authority, and N38 for provenance, and N42 for foreign functions, and N49
    // for escapes and the syntax beside them, N50 for function values, N51 for effect
    // parameters, and N52 for declassification, N53–N55 for channels, the clock and FFI v2,
    // N62 for device registers, N66 for `ints_sum`, N76 for Unicode escapes, N78 for traits, N79 for attenuation, N80 for provenance v3, N85 for FFI v3, N86 for the
    // device registers by width, N87 for contracts, N103 for re-exports, N104 for authority
    // without inheritance, Gate 1-C1 for closure environments in the ownership graph and
    // Gate 2 for numbers, again for arrays and constants, and again for bytes and text, and M1
    // for imported impl methods' effects, and Gate 2 for impls of every numeric type and for
    // files and handles, and for networking.
    assert_eq!(nazm_cache::SEMANTIC_EPOCH, 37);
}
