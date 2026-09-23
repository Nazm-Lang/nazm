//! Native object reuse: what is reused, what is not, and what may never be.
//!
//! # The two halves
//!
//! **Precision.** An object is keyed on the bytes handed to the object compiler, so the
//! questions here are about emitted IR rather than about source edits: a dependency's body
//! change, an export nobody calls, a comment that moves a position and a comment that does
//! not. Each has one right answer and the key produces it without a rule being written
//! down.
//!
//! **Safety.** A warm store may not change what a program means, may not bypass a
//! whole-program refusal, and may not survive a change of backend, target or optimisation
//! level. Every test of that shape compares against the same build with `--no-cache`,
//! which is the only reason a compiler needs a way to turn its cache off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Write a project, with a marker so every module's identity is measured from its root.
fn project(dir: &Path, files: &[(&str, &str)]) {
    std::fs::write(dir.join("nazm.root"), "").expect("marker");
    for (name, text) in files {
        write(dir, name, text);
    }
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// What one build did with the native cache.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Did {
    units: u64,
    compiled: u64,
    reused: u64,
    rejected: u64,
    stored: u64,
}

/// What one build produced, for the comparisons that are about meaning rather than work.
#[derive(Debug, PartialEq, Eq)]
struct Produced {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn build_in(dir: &Path, root: &str, out: &str, extra: &[&str]) -> (Produced, Option<Did>) {
    build_with(Command::new(NAZM), dir, root, out, extra)
}

fn build_with(
    mut command: Command,
    dir: &Path,
    root: &str,
    out: &str,
    extra: &[&str],
) -> (Produced, Option<Did>) {
    let result = command
        .args(["build", root, "-o", out, "--build-report"])
        .args(extra)
        .current_dir(dir)
        .output()
        .expect("build");
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    let did = stdout
        .lines()
        .find(|l| l.contains("nazm.build-report/1"))
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| {
            let n = |name: &str| v["native"][name].as_u64().unwrap_or(u64::MAX);
            Did {
                units: n("units"),
                compiled: n("compiled"),
                reused: n("reused"),
                rejected: n("rejected"),
                stored: n("stored"),
            }
        });
    (
        Produced {
            ok: result.status.success(),
            stdout: stdout
                .lines()
                .filter(|l| !l.contains("nazm.build-report/1"))
                .collect::<Vec<_>>()
                .join("\n"),
            stderr,
        },
        did,
    )
}

/// Build, insisting that it succeeded, and report what it did.
fn build(dir: &Path, root: &str) -> Did {
    let (produced, did) = build_in(dir, root, "prog", &[]);
    assert!(produced.ok, "build failed: {}", produced.stderr);
    did.expect("a report")
}

/// Run a built program.
fn run(dir: &Path, exe: &str) -> Produced {
    let out = Command::new(dir.join(exe))
        .current_dir(dir)
        .output()
        .expect("run");
    Produced {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    }
}

/// Every emitted unit's IR, by artefact name, from a `--emit-ir` build.
fn emitted(dir: &Path, root: &str, out: &str) -> Vec<(String, String)> {
    let (produced, _) = build_in(dir, root, out, &["--emit-ir"]);
    assert!(produced.ok, "build failed: {}", produced.stderr);
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "ll")
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{out}.")))
        {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            found.push((name, std::fs::read_to_string(&path).expect("read")));
        }
    }
    found.sort();
    found
}

/// The one emitted unit that defines something from `module`.
fn unit_of(units: &[(String, String)], module: &str) -> String {
    let want = format!("source_filename = \"{module}\"");
    units
        .iter()
        .find(|(_, ir)| ir.contains(&want))
        .map(|(_, ir)| ir.clone())
        .unwrap_or_else(|| panic!("no unit for {module}"))
}

// ---------------------------------------------------------------------------
// §27, §30: what a rebuild costs
// ---------------------------------------------------------------------------

const A_CALLS_B: &[(&str, &str)] = &[
    (
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { print(int_to_str(twice(21))); 0 }\n",
    ),
    (
        "b.nz",
        "pub fn twice(n: Int) -> Int { n * 2 }\nfn helper() -> Int { 1 }\n",
    ),
];

#[test]
fn an_unchanged_rebuild_compiles_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);

    let cold = build(dir.path(), "a.nz");
    assert_eq!(cold.compiled, cold.units);
    assert_eq!(cold.reused, 0);
    assert_eq!(cold.stored, cold.units);

    let warm = build(dir.path(), "a.nz");
    assert_eq!(
        warm.compiled, 0,
        "a warm build still ran the object compiler"
    );
    assert_eq!(warm.reused, warm.units);
    assert_eq!(warm.stored, 0);
    assert_eq!(warm.rejected, 0);
}

#[test]
fn a_dependency_body_change_leaves_the_caller_s_object_reusable() {
    // N5 measured this and N6 turns it into behaviour: A's unit names B only through a
    // `declare` and a `call`, so B's implementation is not in it and cannot invalidate it.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { n + n }\nfn helper() -> Int { 1 }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(
        after.compiled, 1,
        "only b.nz's unit should have been compiled"
    );
    assert_eq!(after.reused, after.units - 1);
    // `42` from the `print`, then `0`: the entry wrapper prints what `main` returned.
    assert_eq!(run(dir.path(), "prog").stdout, "420");
}

/// A change that moves **only** the ownership traffic still misses.
///
/// N8 made reclamation part of what a unit's IR says, so the question §54 of that
/// milestone asks is whether a stale object from before such a change can be reused. It
/// cannot, and the reason needs no rule of its own: `ObjectKey` is a digest of the exact
/// `.ll` text `clang -c` is given, so IR that gained a retain is different input and
/// different input is a different key.
///
/// The edit here is chosen to move nothing else. `let b = a;` on a sequence adds one
/// `@nz.arr_retain` and one release and changes no signature, no constant and no call —
/// so anything that reused this object would be reusing one compiled without them.
#[test]
fn a_change_that_moves_only_ownership_still_misses() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    // The baseline already uses a sequence, so the *runtime* unit is the same before and
    // after and the only unit that can move is `b.nz`'s.
    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { n * 2 }\n\
         fn helper() -> Int {\n    let a = ints_new();\n    ints_push(a, 1)\n}\n",
    );
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { n * 2 }\n\
         fn helper() -> Int {\n    let a = ints_new();\n    let b = a;\n    \
         ints_push(b, 1)\n}\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(
        after.compiled, 1,
        "the unit whose ownership traffic changed was reused"
    );
    assert_eq!(after.reused, after.units - 1);
    assert_eq!(after.rejected, 0, "a rejection is not the same as a miss");

    // And the IR really does differ in the way the name claims, so the test is about
    // ownership rather than about `b.nz` having changed at all.
    let units = emitted(dir.path(), "a.nz", "prog");
    let b = unit_of(&units, "b.nz");
    assert!(
        b.contains("@nz.arr_retain"),
        "the edit did not add ownership traffic:\n{b}"
    );
}

#[test]
fn an_export_the_caller_never_calls_does_not_invalidate_it() {
    // The test that separates real backend dependency tracking from semantic
    // invalidation: B's `InterfaceHash` moves here, and A's object does not care, because
    // A's IR never mentioned the export that changed.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int) -> Int { n * 2 }\npub fn unused(n: Int) -> Int { n }\nfn helper() -> Int { 1 }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(after.compiled, 1, "an unused export invalidated a caller");
    assert_eq!(after.reused, after.units - 1);
}

#[test]
fn a_called_signature_change_reaches_the_caller_s_own_ir() {
    // The language will not let A keep type-checking while B's exported signature moves,
    // so the two are edited together and the evidence is the emitted IR: A's `declare`
    // and `call` carry B's ABI, which is exactly the dependency that must invalidate it.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    let before = unit_of(&emitted(dir.path(), "a.nz", "one"), "a.nz");
    assert!(
        before.contains("declare i64 @nz.m.b_2Enz.twice(i64)"),
        "{before}"
    );

    write(
        dir.path(),
        "b.nz",
        "pub fn twice(n: Int, m: Int) -> Int { n * m }\nfn helper() -> Int { 1 }\n",
    );
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\nfn main() -> Int { print(int_to_str(twice(21, 2))); 0 }\n",
    );
    let after = unit_of(&emitted(dir.path(), "a.nz", "two"), "a.nz");
    assert!(
        after.contains("declare i64 @nz.m.b_2Enz.twice(i64, i64)"),
        "{after}"
    );
    assert_ne!(
        before, after,
        "a called ABI change left the caller's IR alone"
    );
}

// ---------------------------------------------------------------------------
// §31: an edit that does not reach the IR
// ---------------------------------------------------------------------------

#[test]
fn a_comment_reuses_the_object_when_it_moves_nothing_the_ir_carries() {
    // A module with no fallible operation emits no position, so nothing in it can move.
    // The semantic cache still misses — the source bytes changed — and that is the point:
    // the two caches are keyed on different things and are allowed to disagree.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            ("a.nz", "use \"b.nz\";\nfn main() -> Int { id(0) }\n"),
            ("b.nz", "pub fn id(n: Int) -> Int { n }\n"),
        ],
    );
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "// a comment, and nothing else\npub fn id(n: Int) -> Int { n }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(after.compiled, 0, "an inert comment recompiled something");
    assert_eq!(after.reused, after.units);
}

#[test]
fn a_comment_recompiles_when_it_moves_a_position_the_ir_carries() {
    // The other outcome, and equally correct. `int_to_str` can fail, so its position is
    // written into the unit as text; a line inserted above it moves that text, so the
    // input to the object compiler is a different input. Nothing special-cases comments.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\n// a comment above the call\nfn main() -> Int { print(int_to_str(twice(21))); 0 }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(after.compiled, 1);
    assert_eq!(after.reused, after.units - 1);
}

// ---------------------------------------------------------------------------
// §42, §43, §44: configuration and backend isolation
// ---------------------------------------------------------------------------

#[test]
fn two_optimisation_levels_do_not_share_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);

    let zero = build(dir.path(), "a.nz");
    assert_eq!(zero.compiled, zero.units);

    let (ok, two) = build_in(dir.path(), "a.nz", "prog2", &["--opt-level", "2"]);
    assert!(ok.ok, "{}", ok.stderr);
    let two = two.expect("a report");
    assert_eq!(
        two.reused, 0,
        "an -O2 build reused an -O0 object, so two configurations share a key"
    );
    assert_eq!(two.compiled, two.units);

    // And each level keeps its own entries: both are warm afterwards.
    assert_eq!(build(dir.path(), "a.nz").compiled, 0);
    let (_, again) = build_in(dir.path(), "a.nz", "prog2", &["--opt-level", "2"]);
    assert_eq!(again.expect("a report").compiled, 0);
}

/// A `clang` on `PATH` that forwards to the real one but calls itself something else.
///
/// Changing the reported identity is the whole of the test: no second toolchain has to be
/// installed, and nothing inside the compiler is reached into. It is the same mechanism a
/// user's `PATH` uses, which is the mechanism the identity is supposed to follow.
fn shimmed(dir: &Path, version: &str) -> Command {
    let real = String::from_utf8_lossy(
        &Command::new("sh")
            .args(["-c", "command -v clang"])
            .output()
            .expect("locate clang")
            .stdout,
    )
    .trim()
    .to_owned();
    let bin = dir.join("shim");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let script = format!(
        "#!/bin/sh\nREAL='{real}'\nfor a in \"$@\"; do\n  if [ \"$a\" = '-###' ]; then\n    \
         err=$(\"$REAL\" \"$@\" 2>&1); code=$?\n    printf '%s\\n' \"$err\" | sed '1s|.*|{version}|' >&2\n    \
         exit $code\n  fi\ndone\nexec \"$REAL\" \"$@\"\n"
    );
    std::fs::write(bin.join("clang"), script).expect("shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(bin.join("clang"), std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let mut c = Command::new(NAZM);
    c.env("PATH", format!("{}:{path}", bin.display()));
    c
}

#[test]
fn a_backend_that_calls_itself_something_else_reuses_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    let cold = build(dir.path(), "a.nz");
    assert_eq!(cold.stored, cold.units);

    let shim = tempfile::tempdir().expect("tempdir");
    let (ok, did) = build_with(
        shimmed(shim.path(), "Pretend clang version 0.0.0"),
        dir.path(),
        "a.nz",
        "prog",
        &[],
    );
    assert!(ok.ok, "{}", ok.stderr);
    let did = did.expect("a report");
    assert_eq!(
        did.reused, 0,
        "another backend's objects were reused, so the identity is not in the key"
    );
    assert_eq!(did.compiled, did.units);

    // The first backend's entries are untouched, so its next build is still warm.
    assert_eq!(build(dir.path(), "a.nz").compiled, 0);
}

// ---------------------------------------------------------------------------
// §33, §34: identity that does not depend on where or from what
// ---------------------------------------------------------------------------

#[test]
fn two_checkouts_of_one_project_share_objects() {
    // Nothing about where a checkout sits reaches the emitted IR, and nothing about it
    // reaches the resolved codegen configuration, so the work done in one is reused in
    // the other. They share a store here only because they must be made to: the default
    // store lives beside the project, which is the point of it.
    let shared = tempfile::tempdir().expect("tempdir");
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    project(a.path(), A_CALLS_B);
    project(b.path(), A_CALLS_B);

    let with_shared = |dir: &Path| {
        let mut c = Command::new(NAZM);
        c.env("NAZM_CACHE_DIR", shared.path());
        let (ok, did) = build_with(c, dir, "a.nz", "prog", &[]);
        assert!(ok.ok, "{}", ok.stderr);
        did.expect("a report")
    };

    let first = with_shared(a.path());
    assert_eq!(first.compiled, first.units);
    let second = with_shared(b.path());
    assert_eq!(
        second.compiled, 0,
        "a different directory should not be a different program"
    );
    assert_eq!(second.reused, second.units);
}

const TWO_ROOTS: &[(&str, &str)] = &[
    (
        "app.nz",
        "use \"lib/common.nz\";\nfn main() -> Int { shared(1) }\n",
    ),
    (
        "tool/run.nz",
        "use \"../lib/common.nz\";\nfn main() -> Int { shared(2) }\n",
    ),
    ("lib/common.nz", "pub fn shared(n: Int) -> Int { n }\n"),
];

#[test]
fn a_module_two_entry_points_import_is_compiled_once() {
    // N5 proved the two compilations generate identical code for the shared module. This
    // is that turned into reuse: the second executable does not compile it again.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), TWO_ROOTS);

    let first = build(dir.path(), "app.nz");
    assert_eq!(first.compiled, first.units);

    let (ok, second) = build_in(dir.path(), "tool/run.nz", "tool-prog", &[]);
    assert!(ok.ok, "{}", ok.stderr);
    let second = second.expect("a report");
    assert!(
        second.reused >= 1,
        "the shared module was compiled again: {second:?}"
    );

    // And the decisive evidence, independent of any counter: the unit is the same bytes.
    let from_app = unit_of(&emitted(dir.path(), "app.nz", "x"), "lib/common.nz");
    let from_tool = unit_of(&emitted(dir.path(), "tool/run.nz", "y"), "lib/common.nz");
    assert_eq!(from_app, from_tool);
}

#[test]
fn a_different_root_gets_a_different_entry_object() {
    // The entry wrapper names the root module's `main`, so two roots are two entry units
    // and neither may be handed the other's object. No mechanism beyond the generic key
    // is involved: the IR differs, so the key differs.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), TWO_ROOTS);
    let app = emitted(dir.path(), "app.nz", "x");
    let tool = emitted(dir.path(), "tool/run.nz", "y");
    let entry = |units: &[(String, String)]| {
        units
            .iter()
            .find(|(_, ir)| ir.contains("define i32 @main("))
            .map(|(_, ir)| ir.clone())
            .expect("an entry unit")
    };
    assert_ne!(entry(&app), entry(&tool));
}

// ---------------------------------------------------------------------------
// §35, §36, §37: graph shapes
// ---------------------------------------------------------------------------

#[test]
fn three_modules_with_one_private_name_keep_three_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nuse \"c.nz\";\nfn main() -> Int { ea() + eb() + ec() }\n",
            ),
            (
                "a.nz",
                "fn helper() -> Int { 1 }\npub fn ea() -> Int { helper() }\n",
            ),
            (
                "b.nz",
                "fn helper() -> Int { 2 }\npub fn eb() -> Int { helper() }\n",
            ),
            (
                "c.nz",
                "fn helper() -> Int { 3 }\npub fn ec() -> Int { helper() }\n",
            ),
        ],
    );
    let cold = build(dir.path(), "main.nz");
    assert_eq!(cold.units, 6, "four modules, a runtime and an entry");
    assert_eq!(cold.stored, 6, "two units shared an object");

    let warm = build(dir.path(), "main.nz");
    assert_eq!(warm.reused, 6);
    // Each unit found its own object, so the program still computes what it should —
    // `1 + 2 + 3`, with each `helper` the one its own module defines.
    assert_eq!(run(dir.path(), "prog").stdout, "6");
}

#[test]
fn a_diamond_compiles_its_base_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "a.nz",
                "use \"b.nz\";\nuse \"c.nz\";\nfn main() -> Int { fb() + fc() }\n",
            ),
            ("b.nz", "use \"d.nz\";\npub fn fb() -> Int { fd() }\n"),
            ("c.nz", "use \"d.nz\";\npub fn fc() -> Int { fd() }\n"),
            ("d.nz", "pub fn fd() -> Int { 2 }\n"),
        ],
    );
    let cold = build(dir.path(), "a.nz");
    assert_eq!(cold.units, 6, "d.nz was counted once per import edge");
    assert_eq!(cold.compiled, 6);
    assert_eq!(build(dir.path(), "a.nz").reused, 6);
}

#[test]
fn a_legal_import_cycle_caches_and_relinks() {
    // Object reuse imposes no ordering, because a linker has no opinion about which
    // object was produced first.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "a.nz",
                "use \"b.nz\";\npub fn ping() -> Int { 1 }\nfn main() -> Int { pong() }\n",
            ),
            ("b.nz", "use \"a.nz\";\npub fn pong() -> Int { ping() }\n"),
        ],
    );
    let cold = build(dir.path(), "a.nz");
    assert_eq!(cold.compiled, cold.units);
    assert_eq!(build(dir.path(), "a.nz").reused, cold.units);
}

// ---------------------------------------------------------------------------
// §16, §38: what a warm store may never bypass
// ---------------------------------------------------------------------------

#[test]
fn a_warm_store_cannot_bypass_the_recursion_refusal() {
    // The refusal is whole-program and it happens **before** any key exists: a key is a
    // digest of emitted IR, so the IR must be produced, so everything required to produce
    // it has already run. A fully warm store does not get to the store at all.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "a.nz",
                "use \"b.nz\";\npub fn ping(n: Int) -> Int { n }\nfn main() -> Int { pong(1) }\n",
            ),
            (
                "b.nz",
                "use \"a.nz\";\npub fn pong(n: Int) -> Int { ping(n) }\n",
            ),
        ],
    );
    build(dir.path(), "a.nz");
    assert_eq!(build(dir.path(), "a.nz").compiled, 0, "not warm");

    // Now `a::ping → b::pong → a::ping`, introduced by editing one body.
    write(
        dir.path(),
        "a.nz",
        "use \"b.nz\";\npub fn ping(n: Int) -> Int { pong(n) }\nfn main() -> Int { pong(1) }\n",
    );
    let before = std::fs::metadata(dir.path().join("prog"))
        .expect("an executable")
        .len();
    let (produced, _) = build_in(dir.path(), "a.nz", "prog", &[]);
    assert!(!produced.ok, "a warm store accepted a recursive program");
    assert!(
        produced.stderr.contains("N0101"),
        "not the backend's refusal: {}",
        produced.stderr
    );
    // And the executable that was there is the one that is still there.
    assert_eq!(
        std::fs::metadata(dir.path().join("prog"))
            .expect("kept")
            .len(),
        before
    );
}

// ---------------------------------------------------------------------------
// §32, §46, §47: a reused object means the same program
// ---------------------------------------------------------------------------

const FAILING: &[(&str, &str)] = &[
    (
        "app.nz",
        "use \"lib/util.nz\";\nfn main() -> Int { boom(1) }\n",
    ),
    (
        "tool/run.nz",
        "use \"../lib/util.nz\";\nfn main() -> Int { boom(2) }\n",
    ),
    (
        "lib/util.nz",
        "fn zero() -> Int { 0 }\npub fn boom(n: Int) -> Int { n / zero() }\n",
    ),
];

#[test]
fn a_runtime_failure_from_a_reused_object_still_names_its_own_module() {
    // N5 fixed a real defect here — every failure used to name the file `nazm build` was
    // invoked on — and a cache is exactly the mechanism that could bring it back, by
    // handing one executable's object to another.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), FAILING);

    let (ok, _) = build_in(dir.path(), "app.nz", "app", &[]);
    assert!(ok.ok, "{}", ok.stderr);
    let first = run(dir.path(), "app");

    let (ok, did) = build_in(dir.path(), "tool/run.nz", "tool-run", &[]);
    assert!(ok.ok, "{}", ok.stderr);
    assert!(
        did.expect("a report").reused >= 1,
        "the failing module was not reused, so this proves nothing"
    );
    let second = run(dir.path(), "tool-run");

    assert_eq!(
        first.stderr,
        "nazm: error[N0401]: division by zero at lib/util.nz:2:32"
    );
    assert_eq!(second.stderr, first.stderr);
}

#[test]
fn cold_warm_and_disabled_builds_produce_the_same_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);

    let (cold, _) = build_in(dir.path(), "a.nz", "cold", &["--no-cache"]);
    assert!(cold.ok, "{}", cold.stderr);
    let disabled = run(dir.path(), "cold");

    build(dir.path(), "a.nz");
    let first = run(dir.path(), "prog");
    let warm = build(dir.path(), "a.nz");
    assert_eq!(warm.compiled, 0);
    let second = run(dir.path(), "prog");

    assert_eq!(disabled, first);
    assert_eq!(first, second);
}

#[test]
fn deleting_the_whole_store_changes_nothing() {
    // No build correctness may point into derived data. Deleting it costs time and
    // nothing else, which is what makes it a cache.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    build(dir.path(), "a.nz");
    let before = run(dir.path(), "prog");

    std::fs::remove_dir_all(dir.path().join(".nazm")).expect("delete the store");
    let again = build(dir.path(), "a.nz");
    assert_eq!(
        again.compiled, again.units,
        "something survived the deletion"
    );
    assert_eq!(run(dir.path(), "prog"), before);
}

#[test]
fn a_damaged_object_is_recompiled_rather_than_linked() {
    // The end-to-end form of the store's own tests: a wrong byte in a cached object must
    // become a miss, not a linker failure and not a program.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    let cold = build(dir.path(), "a.nz");
    let before = run(dir.path(), "prog");

    let mut damaged = 0;
    let mut stack = vec![dir.path().join(".nazm").join("native")];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let mut raw = std::fs::read(&path).expect("read");
                let last = raw.len() - 1;
                raw[last] ^= 0xff;
                std::fs::write(&path, raw).expect("damage");
                damaged += 1;
            }
        }
    }
    assert_eq!(
        Ok(damaged),
        usize::try_from(cold.units),
        "nothing was damaged"
    );

    let after = build(dir.path(), "a.nz");
    assert_eq!(after.rejected, after.units, "a damaged object was believed");
    assert_eq!(after.compiled, after.units);
    assert_eq!(run(dir.path(), "prog"), before);
}

// ---------------------------------------------------------------------------
// §39, §40, §57: the support artefacts
// ---------------------------------------------------------------------------

#[test]
fn two_programs_needing_the_same_support_share_one_runtime_object() {
    // The runtime unit's content is a whole-program fact, but a small one: which helper
    // sets the program reached. Two programs that reached the same sets emit the same
    // bytes and therefore share an object, with no rule about runtimes anywhere.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            ("one.nz", "fn main() -> Int { 1 }\n"),
            ("two.nz", "fn main() -> Int { 2 }\n"),
        ],
    );
    let first = build(dir.path(), "one.nz");
    assert_eq!(first.compiled, first.units);

    let (ok, second) = build_in(dir.path(), "two.nz", "prog2", &[]);
    assert!(ok.ok, "{}", ok.stderr);
    let second = second.expect("a report");
    assert_eq!(
        second.reused, 1,
        "exactly the runtime should be shared: {second:?}"
    );
}

#[test]
fn every_unit_declares_the_whole_runtime_so_an_abi_change_reaches_all_of_them() {
    // A caller compiled against an old runtime ABI is unsafe even when the runtime object
    // itself is rebuilt. It cannot happen here: each unit carries a declaration of every
    // helper whether or not it calls one, so any change to a helper's signature changes
    // every unit's bytes and therefore every unit's key.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);
    let units = emitted(dir.path(), "a.nz", "prog");

    let declarations: Vec<String> = unit_of(&units, "a.nz")
        .lines()
        // The runtime's helpers, not the Nazm definitions a unit imports: those carry a
        // module in their name and belong to the dependency tests above.
        .filter(|l| {
            l.starts_with("declare")
                && l.contains("@nz.")
                && !l.contains("@nz.m.")
                && !l.contains("@nz.u.")
        })
        .map(str::to_owned)
        .collect();
    // The decisive one: `b.nz` has no channel, no task and no failure, and declares the
    // helper behind every one of them anyway.
    for expected in [
        "declare void @nz.fail(ptr, i64)",
        "declare i1 @nz.chan_recv(ptr, ptr, ptr, i64)",
        "declare i32 @nz.spawn(ptr, ptr, ptr)",
    ] {
        assert!(
            declarations.iter().any(|d| d == expected),
            "a.nz is missing {expected}: {declarations:?}"
        );
    }
    // And the two units declare the same runtime, so a change to any helper's signature
    // is a change to both of their keys.
    let b = unit_of(&units, "b.nz");
    for d in &declarations {
        assert!(b.contains(d.as_str()), "b.nz is missing {d}");
    }
}

// ---------------------------------------------------------------------------
// §41: the two caches are independent in both directions
// ---------------------------------------------------------------------------

#[test]
fn a_semantic_hit_does_not_imply_a_native_hit() {
    // A warm semantic store says the program still means what it meant. It says nothing
    // about a target or an optimisation level, and it must not be allowed to.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_CALLS_B);

    let check = |extra: &[&str]| {
        let out = Command::new(NAZM)
            .args(["check", "a.nz", "--cache-report"])
            .args(extra)
            .current_dir(dir.path())
            .output()
            .expect("check");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text
            .lines()
            .find(|l| l.contains("nazm.check-report/1"))
            .expect("a report")
            .to_owned();
        serde_json::from_str::<Value>(&line).expect("a report")["reused"]
            .as_u64()
            .unwrap_or(u64::MAX)
    };

    assert_eq!(check(&[]), 0, "cold");
    assert_eq!(check(&[]), 2, "the semantic store is warm");

    // Warm semantically, and entirely cold natively, because no object for this
    // configuration has ever been produced.
    let (ok, did) = build_in(dir.path(), "a.nz", "prog2", &["--opt-level", "2"]);
    assert!(ok.ok, "{}", ok.stderr);
    let did = did.expect("a report");
    assert_eq!(did.reused, 0);
    assert_eq!(did.compiled, did.units);

    // And the reverse, which the comment tests above establish from the other side: the
    // semantic store is still warm after a build that reused every object.
    assert_eq!(build(dir.path(), "a.nz").compiled, did.units);
    assert_eq!(build(dir.path(), "a.nz").compiled, 0);
    assert_eq!(check(&[]), 2);
}

// --- records, and the boundary they must not blur ---------------------------------------

/// A calls B, and B exports a record.
const A_USES_B_RECORD: &[(&str, &str)] = &[
    (
        "a.nz",
        "use \"b.nz\";\n\
         fn main() -> Int { let p = at(21); print(int_to_str(p.x + p.x)); 0 }\n",
    ),
    (
        "b.nz",
        "pub struct Point {\n    x: Int,\n}\n\
         struct Hidden {\n    n: Int,\n}\n\
         fn secret() -> Int { Hidden(n: 0).n }\n\
         pub fn at(n: Int) -> Point { Point(x: n + secret()) }\n",
    ),
];

#[test]
fn a_record_program_is_compiled_cold_and_reused_warm() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_USES_B_RECORD);
    let first = build(dir.path(), "a.nz");
    assert_eq!(first.reused, 0);
    assert_eq!(run(dir.path(), "prog").stdout, "420");

    let second = build(dir.path(), "a.nz");
    assert_eq!(second.compiled, 0, "nothing changed, so nothing to compile");
    assert_eq!(second.reused, second.units);
}

#[test]
fn a_private_record_added_to_a_dependency_leaves_the_caller_s_object_reusable() {
    // N6's boundary, asked of records: A's unit names B through a `declare` and a `call`,
    // and a record B keeps to itself is not in A's text — so it cannot move A's key.
    // Nothing about `ObjectKey` had to learn what a record is for this to hold.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_USES_B_RECORD);
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub struct Point {\n    x: Int,\n}\n\
         struct Hidden {\n    n: Int,\n    extra: Str,\n}\n\
         struct Another {\n    m: Int,\n}\n\
         fn secret() -> Int { str_len(Hidden(n: 0, extra: \"\").extra) + Another(m: 0).m }\n\
         pub fn at(n: Int) -> Point { Point(x: n + secret()) }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(
        after.compiled, 1,
        "only b.nz's unit should have been compiled"
    );
    assert_eq!(after.reused, after.units - 1);
    assert_eq!(run(dir.path(), "prog").stdout, "420");
}

#[test]
fn a_record_a_unit_never_mentions_is_not_in_its_text() {
    // The other half of the same boundary, read out of the IR rather than inferred from
    // a cache hit: a unit carries the record types **it uses**, closed over field types,
    // and nothing else. A record declared in another module and never named here would
    // otherwise put that module's edits into this module's key.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            ("a.nz", "use \"b.nz\";\nfn main() -> Int { plain() }\n"),
            (
                "b.nz",
                "pub struct Unused {\n    x: Int,\n}\n\
                 pub fn plain() -> Int { Unused(x: 1).x }\n",
            ),
        ],
    );
    let units = emitted(dir.path(), "a.nz", "prog");
    let a = unit_of(&units, "a.nz");
    let b = unit_of(&units, "b.nz");
    assert!(
        !a.contains("nz.t."),
        "a.nz names no record and should carry no record type:\n{a}"
    );
    assert!(
        b.contains("nz.t."),
        "b.nz uses `Unused` and must define its type:\n{b}"
    );
}

#[test]
fn a_record_type_is_named_from_its_durable_identity_and_not_its_position() {
    // Two modules each declare a private `Thing`. Both units must lay one out, and the
    // two names must differ — a name derived from a declaration index or a load order
    // would have made them one type to the linker, which is the miscompile N5 removed for
    // functions and this is the same removal for types.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { from_a() + from_b() }\n",
            ),
            (
                "a.nz",
                "struct Thing {\n    n: Int,\n}\npub fn from_a() -> Int { Thing(n: 1).n }\n",
            ),
            (
                "b.nz",
                "struct Thing {\n    m: Str,\n}\npub fn from_b() -> Int { str_len(Thing(m: \"ab\").m) }\n",
            ),
        ],
    );
    let units = emitted(dir.path(), "main.nz", "prog");
    let a = unit_of(&units, "a.nz");
    let b = unit_of(&units, "b.nz");
    let name_in = |text: &str| -> String {
        text.lines()
            .find(|l| l.contains("= type {"))
            .unwrap_or_else(|| panic!("no record type in:\n{text}"))
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let (an, bn) = (name_in(&a), name_in(&b));
    assert_ne!(an, bn, "two modules' `Thing` share a native name");
    assert!(an.contains("a_2Enz"), "{an}");
    assert!(bn.contains("b_2Enz"), "{bn}");
}

#[test]
fn reordering_a_records_fields_changes_no_object() {
    // `docs/spec.md` says field order is not semantic; the layout is canonical by field
    // name, so the claim holds of the object file too. A source edit that means nothing
    // must not produce a different binary, or the compiler and the linker would disagree
    // about whether anything happened.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[(
            "m.nz",
            "struct Point {\n    x: Int,\n    label: Str,\n}\n\
             fn main() -> Int { let p = Point(x: 1, label: \"ab\"); p.x + str_len(p.label) }\n",
        )],
    );
    let before = emitted(dir.path(), "m.nz", "prog");
    write(
        dir.path(),
        "m.nz",
        "struct Point {\n    label: Str,\n    x: Int,\n}\n\
         fn main() -> Int { let p = Point(x: 1, label: \"ab\"); p.x + str_len(p.label) }\n",
    );
    let after = emitted(dir.path(), "m.nz", "prog");
    assert_eq!(
        unit_of(&before, "m.nz"),
        unit_of(&after, "m.nz"),
        "reordering two field declarations changed the emitted IR"
    );
}

// --- enums, and the boundary they must not blur ------------------------------------------

/// A calls B, and B exports an enum A matches on.
const A_USES_B_ENUM: &[(&str, &str)] = &[
    (
        "a.nz",
        "use \"b.nz\";\n\
         fn main() -> Int { let s = start(21); \
         print(int_to_str(match s { State.Ready() => 0, State.Done(code: c) => c * 20, })); 0 }\n",
    ),
    (
        "b.nz",
        "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
         enum Hidden {\n    A,\n}\n\
         fn secret() -> Int { match Hidden.A() { Hidden.A() => 0, } }\n\
         pub fn start(n: Int) -> State { State.Done(code: n + secret()) }\n",
    ),
];

#[test]
fn an_enum_program_is_compiled_cold_and_reused_warm() {
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_USES_B_ENUM);
    let first = build(dir.path(), "a.nz");
    assert_eq!(first.reused, 0);
    assert_eq!(run(dir.path(), "prog").stdout, "4200");

    let second = build(dir.path(), "a.nz");
    assert_eq!(second.compiled, 0, "nothing changed, so nothing to compile");
    assert_eq!(second.reused, second.units);
}

#[test]
fn a_private_enum_added_to_a_dependency_leaves_the_caller_s_object_reusable() {
    // §112 and §113. `ObjectKey` is keyed on the text a unit actually produces, and an
    // enum B keeps to itself is not in A's text — so it cannot move A's key. Nothing
    // about `ObjectKey` had to learn what an enum is for this to hold.
    let dir = tempfile::tempdir().expect("tempdir");
    project(dir.path(), A_USES_B_ENUM);
    build(dir.path(), "a.nz");

    write(
        dir.path(),
        "b.nz",
        "pub enum State {\n    Ready,\n    Done(code: Int),\n}\n\
         enum Hidden {\n    A(n: Str),\n    B,\n}\n\
         enum Another {\n    Only,\n}\n\
         fn secret() -> Int { match Hidden.B() { Hidden.A(n: s) => str_len(s), \
         Hidden.B() => 0, } + match Another.Only() { Another.Only() => 0, } }\n\
         pub fn start(n: Int) -> State { State.Done(code: n + secret()) }\n",
    );
    let after = build(dir.path(), "a.nz");
    assert_eq!(
        after.compiled, 1,
        "only b.nz's unit should have been compiled"
    );
    assert_eq!(after.reused, after.units - 1);
    assert_eq!(run(dir.path(), "prog").stdout, "4200");
}

#[test]
fn an_enum_a_unit_never_mentions_is_not_in_its_text() {
    // The other half, read out of the IR: a unit carries the enum types **it uses**,
    // closed over payload types, and nothing else.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            ("a.nz", "use \"b.nz\";\nfn main() -> Int { plain() }\n"),
            (
                "b.nz",
                "pub enum Unused {\n    A(x: Int),\n}\n\
                 pub fn plain() -> Int { match Unused.A(x: 1) { Unused.A(x: n) => n, } }\n",
            ),
        ],
    );
    let units = emitted(dir.path(), "a.nz", "prog");
    let a = unit_of(&units, "a.nz");
    let b = unit_of(&units, "b.nz");
    assert!(
        !a.contains("nz.t."),
        "a.nz names no enum and should carry no type:\n{a}"
    );
    assert!(
        b.contains("nz.t."),
        "b.nz uses `Unused` and must define its type:\n{b}"
    );
}

#[test]
fn reordering_an_enums_variants_changes_no_object() {
    // `docs/spec.md` says variant order is not semantic, so the discriminant is derived
    // from the names — and the claim holds of the object file too. A source edit that
    // means nothing must not produce a different binary. The same for payload fields.
    for (what, before, after) in [
        (
            "variants",
            "enum E {\n    Ready,\n    Done(code: Int, why: Str),\n}\n",
            "enum E {\n    Done(code: Int, why: Str),\n    Ready,\n}\n",
        ),
        (
            "payload fields",
            "enum E {\n    Ready,\n    Done(code: Int, why: Str),\n}\n",
            "enum E {\n    Ready,\n    Done(why: Str, code: Int),\n}\n",
        ),
    ] {
        const BODY: &str = "fn main() -> Int { match E.Done(code: 1, why: \"ab\") { \
                            E.Ready() => 0, E.Done(code: c, why: w) => c + str_len(w), } }\n";
        let dir = tempfile::tempdir().expect("tempdir");
        project(dir.path(), &[("m.nz", &format!("{before}{BODY}"))]);
        let first = emitted(dir.path(), "m.nz", "prog");
        write(dir.path(), "m.nz", &format!("{after}{BODY}"));
        let second = emitted(dir.path(), "m.nz", "prog");
        assert_eq!(
            unit_of(&first, "m.nz"),
            unit_of(&second, "m.nz"),
            "reordering {what} changed the emitted text"
        );
    }
}

#[test]
fn an_enum_type_is_named_from_its_durable_identity_and_not_its_position() {
    // Two modules each declare a private `Thing`. Both units must lay one out, and the
    // two names must differ — the same removal N5 made for functions and N9 for records.
    let dir = tempfile::tempdir().expect("tempdir");
    project(
        dir.path(),
        &[
            (
                "main.nz",
                "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { from_a() + from_b() }\n",
            ),
            (
                "a.nz",
                "enum Thing {\n    N(n: Int),\n}\n\
                 pub fn from_a() -> Int { match Thing.N(n: 1) { Thing.N(n: n) => n, } }\n",
            ),
            (
                "b.nz",
                "enum Thing {\n    M(m: Str),\n}\n\
                 pub fn from_b() -> Int { match Thing.M(m: \"ab\") { Thing.M(m: m) => str_len(m), } }\n",
            ),
        ],
    );
    let units = emitted(dir.path(), "main.nz", "prog");
    let a = unit_of(&units, "a.nz");
    let b = unit_of(&units, "b.nz");
    let names_in = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.contains("= type {"))
            .filter_map(|l| l.split_whitespace().next().map(ToOwned::to_owned))
            .collect()
    };
    let (an, bn) = (names_in(&a), names_in(&b));
    assert!(!an.is_empty() && !bn.is_empty(), "{an:?} {bn:?}");
    for x in &an {
        assert!(
            !bn.contains(x),
            "two modules' `Thing` share a native name: {x}"
        );
    }
    assert!(an.iter().all(|n| n.contains("a_2Enz")), "{an:?}");
    assert!(bn.iter().all(|n| n.contains("b_2Enz")), "{bn:?}");
}
