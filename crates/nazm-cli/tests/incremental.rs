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
            .map(|d| {
                format!(
                    "{}@{}:{}",
                    d["code"].as_str().unwrap_or("?"),
                    d["primary"]["line"].as_u64().unwrap_or(0),
                    d["primary"]["column"].as_u64().unwrap_or(0),
                )
            })
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
    assert_eq!((first.checked, first.reused, first.stored), (2, 0, 2));
    assert_eq!(first.root, "marker");

    let second = check(dir.path(), "a.nz");
    assert_eq!((second.checked, second.reused), (0, 2));
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
        (1, 1, 1),
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
            (1, 1),
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
    assert_eq!((after.checked, after.reused), (2, 0));
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
        assert_eq!(after.reused, 0, "{what}: the importer kept a stale answer");
        assert_eq!(
            meaning(&after),
            meaning(&cold(dir.path(), "a.nz")),
            "{what}"
        );
    }
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
    assert_eq!(check(dir.path(), "a.nz").checked, 3);

    write(
        dir.path(),
        "c.nz",
        "pub fn deep(n: Int) -> Int { inner() + n }\nfn inner() -> Int { 3 + 4 }\n",
    );
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (1, 2));
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
    assert_eq!(check(dir.path(), "a.nz").checked, 3);

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
        (2, 1),
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
    assert_eq!(check(dir.path(), "a.nz").checked, 3);

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
    assert_eq!((after.checked, after.reused), (3, 0));
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
    assert_eq!(first.stored, 0);

    let second = check(dir.path(), "a.nz");
    assert_eq!(second.reused, 0, "a refusal must not be reused");
    assert_eq!(meaning(&first), meaning(&second));
}

#[test]
fn deleting_the_cache_changes_nothing() {
    // §14. It is derived data: removing it costs work and decides nothing.
    let dir = tempfile::tempdir().expect("tempdir");
    two_modules(dir.path(), B);
    let warm_setup = check(dir.path(), "a.nz");
    assert_eq!(warm_setup.stored, 2);

    std::fs::remove_dir_all(dir.path().join(".nazm")).expect("remove the cache");
    let after = check(dir.path(), "a.nz");
    assert_eq!((after.checked, after.reused), (2, 0));
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
fn entry_for(dir: &Path, key: &str) -> std::path::PathBuf {
    entries(dir)
        .into_iter()
        .find(|p| {
            std::fs::read_to_string(p).is_ok_and(|t| t.contains(&format!(r#""module":"{key}""#)))
        })
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
            t.replace("nazm.check/1", "nazm.check/9")
        }),
        ("an edited interface", |t| {
            t.replace(r#""ret":"Int""#, r#""ret":"Str""#)
        }),
        ("an edited source fingerprint", |t| swap_digest(t, "source")),
        ("a foreign checker", |t| swap_digest(t, "checker")),
        ("an edited module key", |t| {
            t.replacen(r#""module":"b.nz""#, r#""module":"z.nz""#, 1)
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
            (1, 1, 1),
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

    assert_eq!(with_shared(a.path()), (true, 2, 0));
    assert_eq!(
        with_shared(b.path()),
        (true, 0, 2),
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
    assert_eq!((first.checked, first.reused), (2, 0));

    let second = check(dir.path(), "tool/run.nz");
    assert_eq!(
        (second.checked, second.reused),
        (1, 1),
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
    assert_eq!((first.checked, first.reused), (2, 0));

    let second = check(dir.path(), "tool/run.nz");
    assert_eq!((second.checked, second.reused), (2, 0));
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
    assert_eq!((first.checked, first.stored), (2, 2));

    let second = check(dir.path(), "two.nz");
    assert_eq!(
        (second.checked, second.reused, second.rejected),
        (2, 0, 0),
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
        second.reused, 0,
        "nothing here should have a key: {second:?}"
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
    assert_eq!((after.checked, after.reused), (0, 2));
    assert!(after.accepted);
}
