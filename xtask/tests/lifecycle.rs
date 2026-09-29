//! The mutation runner's lifecycle, exercised against real processes and a real tree.
//!
//! Everything here runs the shipped `xtask` binary against a **disposable Cargo project**
//! in a temporary directory. Nothing touches this repository: a test that interrupts a
//! mutation run has to leave a damaged tree behind for a moment, and doing that to the
//! checkout is how an afternoon was lost once already.
//!
//! # Why these cannot be unit tests
//!
//! Three of the properties are about processes, not functions.
//!
//! * **Mutual exclusion** needs two independent processes to contend. One process cannot
//!   demonstrate that a *killed* holder's lock is released, because the kernel releases it
//!   on exit and there is no exit to observe from inside.
//! * **Recovery after an interruption** needs a process that did not create the state it
//!   repairs. A hand-built record proves that recovery reads records correctly; it says
//!   nothing about whether a real interrupted run leaves a state recovery can read.
//! * **Verdicts** are produced by building and running a suite, which is `cargo`.
//!
//! # Why they are `#[ignore]`d
//!
//! Each one drives several nested `cargo` invocations. `cargo xtask mutate` runs
//! `cargo test --workspace` once per catalogued defect — 109 of them — so leaving these in
//! the default set would add their cost to every iteration. Run them explicitly:
//!
//! ```sh
//! cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` because several of them spawn `cargo`, and running those in
//! parallel competes for the same build resources for no benefit.

// A test that cannot assert is not a test. The workspace denies these because the
// harness computes numbers a run is judged on; a fixture that panics on a broken
// precondition is exactly what is wanted here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const XTASK: &str = env!("CARGO_BIN_EXE_xtask");

/// The disposable project's one source file.
///
/// Three separate mutation targets, each unique in the file so the runner's
/// "must occur exactly once" check passes:
///
/// * `{ 42 }` — asserted by the test, so changing it is **caught**; replacing it with a
///   string makes the crate **not build**.
/// * `{ 7 }` — never asserted, so changing it **survives**.
/// * `{ 0 }` — the delay the test sleeps for, so changing it makes the mutant *slow*,
///   which is what makes an interruption reliable rather than a race.
const SOURCE: &str = r#"pub fn answer() -> u64 { 42 }

pub fn unchecked() -> u64 { 7 }

pub fn delay_ms() -> u64 { 0 }

#[cfg(test)]
mod tests {
    #[test]
    fn the_answer_is_right() {
        // Announces this process, so a test can prove the descendant is still alive
        // after its coordinator has been killed. Written before the sleep, and holding
        // the pid rather than a word, because "it started" is not "it is running".
        //
        // The delay goes with it, and that is not decoration: the *baseline* run writes
        // this file too, with delay 0, and then exits at once. A test that waited only
        // for the file would read a pid that was already dead and prove nothing.
        if let Ok(p) = std::env::var("NAZM_FIXTURE_READY") {
            let _ = std::fs::write(p, format!("{} {}", std::process::id(), super::delay_ms()));
        }
        // Which build this is: one line per run, the value this binary computes. Lets a
        // test see that every mutant was measured against its own bytes.
        if let Ok(p) = std::env::var("NAZM_FIXTURE_OBSERVE") {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{}", super::answer());
            }
        }
        // A descendant that leaves the group, the one shape the harness cannot reach
        // once its parent is gone. Only under the slow mutant, so the baseline leaves
        // nothing behind.
        if let Ok(p) = std::env::var("NAZM_FIXTURE_ESCAPE") {
            if super::delay_ms() > 0 {
                use std::os::unix::process::CommandExt as _;
                if let Ok(c) = std::process::Command::new("sleep")
                    .arg("120")
                    .process_group(0)
                    .spawn()
                {
                    let _ = std::fs::write(p, c.id().to_string());
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(super::delay_ms()));
        assert_eq!(super::answer(), 42);
    }
}
"#;

const SRC_REL: &str = "src/lib.rs";

/// What the slow mutation sets `delay_ms()` to. Long enough that the window in which the
/// tree is damaged is interrupted deliberately rather than by luck.
const MUTANT_DELAY_MS: u64 = 30_000;

/// Wait until the run is mid-mutation, and return the live descendant's pid.
///
/// "Mid-mutation" is three facts, not one: the record exists, the source holds the
/// expected mutated bytes, and the *mutated* build's test process has announced itself.
/// The third matters because the baseline writes the same announcement file and exits at
/// once — waiting only for the file yields a pid that is already dead.
fn wait_until_mutated(
    src: &Path,
    record: &Path,
    ready: &Path,
    expected_mutated: &str,
    delay_ms: u64,
    deadline: Duration,
) -> u32 {
    let mut descendant = 0u32;
    until(
        "the run to be mid-mutation with a live descendant",
        deadline,
        || {
            let has_record = record.exists();
            let mutated = fs::read_to_string(src).is_ok_and(|s| s == expected_mutated);
            let pid = fs::read_to_string(ready).ok().and_then(|s| {
                let mut f = s.split_whitespace();
                let pid = f.next()?.parse::<u32>().ok()?;
                let d = f.next()?.parse::<u64>().ok()?;
                (d == delay_ms).then_some(pid)
            });
            match (has_record, mutated, pid) {
                (true, true, Some(p)) => {
                    descendant = p;
                    Ok(())
                }
                (r, m, p) => Err(format!(
                    "record={r} mutated={m} live-descendant={}",
                    p.map_or("not yet announced by the mutated build".to_owned(), |v| v
                        .to_string())
                )),
            }
        },
    );
    descendant
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    let mut perm = fs::metadata(path).expect("metadata").permissions();
    perm.set_mode(mode);
    fs::set_permissions(path, perm).expect("set_permissions");
}

/// A standalone Cargo project with no dependencies.
///
/// `[workspace]` is present so the project is its own workspace wherever the temporary
/// directory lands, and no dependencies means `cargo` never reaches the network or the
/// shared registry lock.
fn disposable_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n\n\
         [package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n",
    )
    .expect("manifest");
    fs::write(root.join(SRC_REL), SOURCE).expect("source");
    // Written now rather than by the first build. A campaign's identity is the tree's
    // digest, and a build that adds a file to the tree is refused for changing it.
    fs::write(
        root.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\n\
         version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
    )
    .expect("lockfile");
    dir
}

fn catalogue(root: &Path, name: &str, find: &str, replace: &str) -> PathBuf {
    let path = root.join(format!("{name}.toml"));
    fs::write(
        &path,
        format!(
            "[[mutation]]\n\
             name = \"{name}\"\n\
             why = \"lifecycle fixture\"\n\
             file = \"{SRC_REL}\"\n\
             find = {}\n\
             replace = {}\n",
            toml_string(find),
            toml_string(replace)
        ),
    )
    .expect("catalogue");
    path
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

struct Run {
    status: Option<i32>,
    stdout: String,
}

impl Run {
    fn says(&self, needle: &str) -> bool {
        self.stdout.contains(needle)
    }
}

fn xtask(args: &[&str]) -> Run {
    let out = Command::new(XTASK)
        .args(args)
        .stderr(Stdio::piped())
        .output()
        .expect("run xtask");
    Run {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr),
    }
}

/// Poll until `cond` holds, or fail with what was still missing at the deadline.
///
/// Every wait in this file goes through here. A bare `sleep` would make the tests pass or
/// fail on machine speed, and a pass bought that way is worth nothing.
fn until(what: &str, deadline: Duration, mut cond: impl FnMut() -> Result<(), String>) {
    let end = Instant::now() + deadline;
    let mut last = String::from("never evaluated");
    while Instant::now() < end {
        match cond() {
            Ok(()) => return,
            Err(why) => last = why,
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out after {deadline:?} waiting for {what}: {last}");
}

/// The tree is exactly as it started and the runner left nothing behind.
fn assert_clean(root: &Path) {
    assert_eq!(
        fs::read_to_string(root.join(SRC_REL)).expect("read source"),
        SOURCE,
        "the source was not restored to its original bytes"
    );
    assert!(
        !root.join("target/.nazm-mutation-backup").exists(),
        "a write-ahead record outlived the run that wrote it"
    );
    // Not "the lock file is gone" — it is deliberately kept, as the coordination object
    // the next run locks. What must be true is that it is *released*, and the only
    // honest way to say that is that a later process can take it.
    let r = xtask(&[
        "mutate-recover",
        "--root",
        &root.display().to_string(),
        "--confirm-no-writers",
    ]);
    assert_eq!(r.status, Some(0), "the lock was not released: {}", r.stdout);
    assert!(
        r.says("nothing to recover"),
        "expected a clean tree, got: {}",
        r.stdout
    );
}

// ---------------------------------------------------------------------------
// A. Independent verdicts, each with its cleanup checked immediately.
// ---------------------------------------------------------------------------

/// One entry per run, so a verdict cannot be produced by a neighbouring mutation.
///
/// Cleanup is asserted after *each* run rather than once at the end: a runner that
/// restored only the last file it touched would pass a single check at the end and still
/// be broken.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn one_entry_runs_report_caught_survived_and_did_not_build() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();

    // Caught: the mutation changes a value the suite asserts on.
    let c = catalogue(root, "caught", "u64 { 42 }", "u64 { 43 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &c.display().to_string(),
    ]);
    assert_eq!(
        run.status,
        Some(0),
        "a caught mutation is a pass: {}",
        run.stdout
    );
    assert!(run.says("caught       caught"), "{}", run.stdout);
    assert_clean(root);

    // Survived: the mutation changes a value nothing asserts on.
    let s = catalogue(root, "survived", "u64 { 7 }", "u64 { 8 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &s.display().to_string(),
    ]);
    assert_eq!(run.status, Some(1), "a survivor must fail the run");
    assert!(run.says("SURVIVED     survived"), "{}", run.stdout);
    assert!(
        run.says("injected and compiled, but the suite still passed"),
        "{}",
        run.stdout
    );
    assert_clean(root);

    // Did not build: the mutated tree does not compile, so a failing suite would not
    // mean the defect was detected.
    let b = catalogue(root, "unusable", "u64 { 42 }", "u64 { \\\"forty-two\\\" }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &b.display().to_string(),
    ]);
    assert_eq!(
        run.status,
        Some(1),
        "an unusable mutation must fail the run"
    );
    assert!(run.says("UNUSABLE     unusable"), "{}", run.stdout);
    assert!(run.says("does not compile"), "{}", run.stdout);
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// B. Two independent processes, and only one winner.
// ---------------------------------------------------------------------------

/// Mutual exclusion across processes, and release afterwards.
///
/// Synchronised on files the holder writes *after* it has the lock, never on elapsed
/// time: the competitor must start only once the lock is genuinely held, or the test
/// proves nothing about contention.
#[test]
#[ignore = "spawns competing processes; run with --ignored"]
fn two_independent_processes_never_both_hold_the_tree() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let ready = root.join("held");
    let release = root.join("go");

    let mut holder = Command::new(XTASK)
        .args([
            "mutate-hold",
            "--root",
            &root_s,
            "--ready",
            &ready.display().to_string(),
            "--release",
            &release.display().to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn holder");

    until(
        "the first process to hold the lock",
        Duration::from_secs(30),
        || {
            if ready.exists() {
                Ok(())
            } else {
                Err("its ready file has not appeared".to_owned())
            }
        },
    );

    // A second, entirely independent process asks for the same tree.
    let rival_ready = root.join("held-2");
    let rival = Command::new(XTASK)
        .args([
            "mutate-hold",
            "--root",
            &root_s,
            "--ready",
            &rival_ready.display().to_string(),
            "--release",
            &release.display().to_string(),
        ])
        .output()
        .expect("run rival");

    assert!(
        !rival.status.success(),
        "two processes both took the tree: the rival exited {:?}",
        rival.status.code()
    );
    assert!(
        !rival_ready.exists(),
        "the rival announced ownership it was refused"
    );

    // And the refusal must say it was contention, not something it could retry through.
    let probe = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(probe.status, Some(1));
    assert!(
        probe.says("another mutation run holds"),
        "contention must be named as such: {}",
        probe.stdout
    );

    fs::write(&release, "go").expect("signal release");
    let held = holder.wait().expect("reap holder");
    assert!(held.success(), "the holder exited {:?}", held.code());

    // Released — which is asserted by a later process succeeding, not by the file's
    // absence. The lock file is deliberately still there.
    assert!(
        root.join("target/.nazm-mutation-lock.v2").exists(),
        "the coordination object must survive release"
    );
    let after = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        after.status,
        Some(0),
        "the lock was not released: {}",
        after.stdout
    );
}

// ---------------------------------------------------------------------------
// C. A real run, really interrupted, really recovered.
// ---------------------------------------------------------------------------

/// Kill a running mutation and repair the tree through the production recovery path.
///
/// The claim is narrow and worth stating: **recovery after an actual interruption**.
/// That the record is written *before* the mutation is a separate property, and this test
/// does not establish it — it could not, because it only ever observes states in which
/// both already exist.
#[test]
#[ignore = "kills a real mutation run; run with --ignored"]
fn an_interrupted_real_run_is_recovered_through_the_production_path() {
    use std::os::unix::process::CommandExt as _;

    let project = disposable_project();
    let project_scratch = tempfile::tempdir().expect("scratch");
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);

    // The mutant sleeps for thirty seconds, so the window in which the tree is damaged
    // is long enough to interrupt deliberately rather than by luck. The baseline still
    // runs at full speed, because the delay only exists once the mutation is applied.
    let cat = catalogue(root, "slow", "u64 { 0 }", "u64 { 30_000 }");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    assert_ne!(expected_mutated, SOURCE, "the fixture must actually change");

    let ready = project_scratch.path().join("descendant.pid");
    let mut runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
        ])
        .env("NAZM_FIXTURE_READY", &ready)
        // Its own process group, so the kill below cannot reach the test runner. Since
        // N12.2 the runner's `cargo` is in a group of its own; what reaches it after
        // this kill is the runner's watchdog, and the end of this test checks it did.
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let pgid = runner.id();

    let record = root.join("target/.nazm-mutation-backup");

    // Ready means the record exists, the source holds the mutated bytes, *and* the
    // mutated build's test is running. Each alone is a state the runner passes through,
    // and killing there would test a different thing than the one claimed.
    let descendant = wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        MUTANT_DELAY_MS,
        Duration::from_secs(240),
    );

    // Kill the whole group. Reaping the runner alone would say nothing about the `cargo`
    // it spawned, which is still writing to the tree.
    let killed = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pgid}")])
        .status()
        .expect("kill the process group");
    assert!(killed.success(), "could not signal the process group");

    runner.wait().expect("reap the runner");

    // Reaping the direct child does not establish that its descendants are gone; they
    // were signalled, but "signalled" is not "stopped". Confirm before recovering,
    // because a surviving `cargo` could still write to the very file about to be
    // compared and restored.
    until(
        "every process in the interrupted run's group to stop",
        Duration::from_secs(60),
        || {
            let out = Command::new("pgrep")
                .args(["-g", &pgid.to_string()])
                .output()
                .expect("pgrep");
            let survivors = String::from_utf8_lossy(&out.stdout);
            let survivors: Vec<&str> = survivors.split_whitespace().collect();
            if survivors.is_empty() {
                Ok(())
            } else {
                Err(format!("still running: {survivors:?}"))
            }
        },
    );

    // The mutant's test ran in the runner's `cargo` group, which the kill above did not
    // address. The watchdog the runner armed did, when the runner died.
    until(
        "the watchdog to take the mutant's test with the runner",
        Duration::from_secs(30),
        || {
            if pid_alive(descendant) {
                Err(format!("descendant {descendant} is still running"))
            } else {
                Ok(())
            }
        },
    );

    // The stranded state, inspected before anything repairs it.
    assert!(
        record.exists(),
        "an interrupted run left no recovery record, so nothing could repair it"
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "the interruption was expected to leave the mutation applied"
    );

    // Recover through the production path. Note what changed: a real run no longer does
    // this for itself, because it cannot establish that the descendants above have
    // stopped. This test *did* establish it, one assertion ago, which is the only reason
    // the attestation flag is honest here.
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "recovery must name what it repaired: {}",
        recovered.stdout
    );

    assert_clean(root);
}

/// The window between restoring a file and clearing its record, reached for real.
///
/// A run killed there leaves an intact tree and a stale record. Recovery must recognise
/// that and clear it, rather than reporting a conflict on a tree that needs no repair —
/// which is what the first version did. The unit test covers the same case on a
/// hand-built record; this one gets there through an actual run.
#[test]
#[ignore = "drives a nested cargo build; run with --ignored"]
fn a_record_stranded_after_the_source_was_restored_is_cleared_not_disputed() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();

    // A complete, successful run: the source ends up restored and the record removed.
    let c = catalogue(root, "caught", "u64 { 42 }", "u64 { 43 }");
    let run = xtask(&[
        "mutate-run",
        "--root",
        &root_s,
        "--catalogue",
        &c.display().to_string(),
    ]);
    assert_eq!(run.status, Some(0), "{}", run.stdout);

    // Now reconstruct exactly the state a kill between those two writes leaves: the file
    // is already original, and the record the run had written is still there. Taken from
    // the real run's own bytes rather than composed by hand.
    let record = root.join("target/.nazm-mutation-backup");
    fs::write(
        &record,
        format!(
            "nazm-mutation-backup 2\n{SRC_REL}\n{}\n{}\n{SOURCE}",
            fnv("pub fn answer() -> u64 { 43 }"),
            fnv(SOURCE)
        ),
    )
    .expect("re-strand the record");

    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "an already-restored tree is not a conflict: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("already-restored {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

/// FNV-1a, matching the runner's own digest.
///
/// Duplicated rather than exported: a test that imports the function under test to
/// compute its expected value checks only that the function equals itself.
fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// D. A descendant that outlives its coordinator.
// ---------------------------------------------------------------------------

/// The case the lock does **not** cover, and the reason recovery is no longer automatic.
///
/// Killing a mutation run releases its lock immediately — Rust opens files `O_CLOEXEC`,
/// so the `cargo` and `rustc` processes it spawned never held the lock and are not killed
/// with it. They keep running and keep writing. A digest of the source does not help: it
/// says what the file held at the instant it was read, not that nothing will write it
/// next.
///
/// So this test kills **only the coordinator**, proves a descendant is still alive, and
/// requires the next run through the production entry point to refuse. It deliberately
/// does *not* stop the descendant first — doing the missing safety step inside the test
/// and then asserting success would be the test proving a property the product does not
/// have.
///
/// Since N12.2 the runner's watchdog kills its `cargo` group when the coordinator dies,
/// so an ordinary descendant no longer outlives it. The one that still does is a
/// descendant that **left the group** and was re-parented when its parent died, which
/// no mechanism here can find. That is the descendant this test uses, and it is why the
/// refusal is still the right behaviour.
#[test]
#[ignore = "kills a real mutation run and leaves a descendant running; run with --ignored"]
fn a_second_run_refuses_while_a_descendant_of_the_dead_coordinator_survives() {
    use std::os::unix::process::CommandExt as _;

    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);
    let ready = root.join("descendant.pid");

    let cat = catalogue(root, "slow", "u64 { 0 }", "u64 { 30_000 }");
    let cat_s = cat.display().to_string();
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    let record = root.join("target/.nazm-mutation-backup");

    // Its own group, so cleanup at the end can reach every descendant. The kill in the
    // middle of this test is aimed at the coordinator alone, by pid.
    let escaped_file = root.join("escaped.pid");
    let mut runner = Command::new(XTASK)
        .args(["mutate-run", "--root", &root_s, "--catalogue", &cat_s])
        .env("NAZM_FIXTURE_READY", &ready)
        .env("NAZM_FIXTURE_ESCAPE", &escaped_file)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let pgid = runner.id();

    // 2. Ready means all three: the record, the mutated bytes, and a descendant that has
    //    announced itself. Waiting on fewer would leave which-state-we-killed-in to luck.
    let descendant = wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        MUTANT_DELAY_MS,
        Duration::from_secs(240),
    );

    let mut escaped = 0u32;
    until(
        "the escaped descendant to announce itself",
        Duration::from_secs(30),
        || match fs::read_to_string(&escaped_file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
        {
            Some(p) => {
                escaped = p;
                Ok(())
            }
            None => Err("not yet".to_owned()),
        },
    );
    assert!(descendant > 0);

    // 3. Only the coordinator. Not the group.
    runner.kill().expect("kill the coordinator");
    runner.wait().expect("reap the coordinator");

    // 4. And a descendant is still there. Not the test process, which the watchdog
    //    takes with the coordinator's `cargo` group, but the one that left the group.
    until(
        "the watchdog to take the in-group descendant",
        Duration::from_secs(30),
        || {
            if pid_alive(descendant) {
                Err(format!("{descendant} still running"))
            } else {
                Ok(())
            }
        },
    );
    assert!(
        pid_alive(escaped),
        "the escaped descendant exited on its own; this test proves nothing unless it \
         outlives its coordinator"
    );

    // 5. A second run, through the production entry point, with that descendant running.
    let second = xtask(&["mutate-run", "--root", &root_s, "--catalogue", &cat_s]);

    // 6. It must refuse. The lock is free — the kernel released it when the coordinator
    //    died — so nothing about the lock stops this; the refusal has to come from the
    //    stranded record, and it has to be a refusal rather than a repair.
    assert_ne!(
        second.status,
        Some(0),
        "a second run started: {}",
        second.stdout
    );
    assert!(
        second.says("recovery is not automatic"),
        "the refusal must say why: {}",
        second.stdout
    );
    assert!(
        second.says("pgrep"),
        "and must give a procedure, not a suggestion: {}",
        second.stdout
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "the refused run restored the source anyway, under a live writer"
    );
    assert!(
        record.exists(),
        "the refused run cleared the recovery record"
    );

    // Recovery without the attestation must refuse on the same grounds.
    let unattested = xtask(&["mutate-recover", "--root", &root_s]);
    assert_ne!(unattested.status, Some(0), "recovery ran unattested");
    assert!(
        unattested.says("recovery is not automatic"),
        "{}",
        unattested.stdout
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read source"),
        expected_mutated,
        "unattested recovery restored anyway"
    );
    assert!(descendant > 0);

    // 7. Now clean up what this test started, and only that: the runner's group, and
    //    the one process that left it, by the pid it announced.
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pgid}")])
        .status();
    let _ = Command::new("kill")
        .args(["-KILL", &escaped.to_string()])
        .status();
    until(
        "the escaped descendant to stop",
        Duration::from_secs(30),
        || {
            if pid_alive(escaped) {
                Err(format!("{escaped} still running"))
            } else {
                Ok(())
            }
        },
    );
    until(
        "the test's own process group to stop",
        Duration::from_secs(60),
        || {
            let out = Command::new("pgrep")
                .args(["-g", &pgid.to_string()])
                .output()
                .expect("pgrep");
            let s = String::from_utf8_lossy(&out.stdout);
            let alive: Vec<&str> = s.split_whitespace().collect();
            if alive.is_empty() {
                Ok(())
            } else {
                Err(format!("still running: {alive:?}"))
            }
        },
    );

    // Only now is the attestation true, and only now does recovery run.
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

fn pid_alive(pid: u32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------------------
// E. A restore that fails, at the real boundary, in a real run.
// ---------------------------------------------------------------------------

/// The injection cannot be taken back out, and the run must stop rather than continue.
///
/// The failure is injected where it actually happens — the source file is made read-only
/// while the mutation is applied, so the runner's own `fs::write` fails — rather than
/// through a filesystem abstraction. Two catalogue entries, so "the run stopped" is an
/// observation about the second one never being attempted and not an inference.
#[test]
#[ignore = "drives a real mutation run; run with --ignored"]
fn a_failed_restore_keeps_the_record_and_abandons_the_rest_of_the_catalogue() {
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);
    let ready = root.join("descendant.pid");
    let record = root.join("target/.nazm-mutation-backup");

    // Short, because this one is not interrupted — it just has to still be inside the
    // mutated window when the file is made unwritable.
    const DELAY: u64 = 3_000;
    let cat = root.join("two.toml");
    fs::write(
        &cat,
        format!(
            "[[mutation]]\n\
             name = \"slow\"\nwhy = \"fixture\"\nfile = \"{SRC_REL}\"\n\
             find = \"u64 {{ 0 }}\"\nreplace = \"u64 {{ {DELAY} }}\"\n\n\
             [[mutation]]\n\
             name = \"never-attempted\"\nwhy = \"must never be attempted\"\nfile = \"{SRC_REL}\"\n\
             find = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 43 }}\"\n"
        ),
    )
    .expect("catalogue");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", &format!("u64 {{ {DELAY} }}"), 1);

    let runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
        ])
        .env("NAZM_FIXTURE_READY", &ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");

    wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        DELAY,
        Duration::from_secs(240),
    );

    // Injected at the restoration boundary: the runner's next write to this file is the
    // one that puts the original back, and it will now fail.
    set_mode(&src, 0o444);

    // Unless this process is root, as it is inside the contained runner: permission bits
    // do not stop root writing, so the failure cannot be injected this way and the test
    // would be asserting on a restore that correctly succeeded. Said, not silently passed.
    if fs::OpenOptions::new().write(true).open(&src).is_ok() {
        set_mode(&src, 0o644);
        let _ = runner.wait_with_output();
        eprintln!(
            "SKIPPED: running as a user that can write a 0444 file (root), so a failing \
             restore cannot be injected by permissions here. Run on the host."
        );
        return;
    }

    let out = runner.wait_with_output().expect("reap runner");
    let said = String::from_utf8_lossy(&out.stdout).into_owned();

    assert!(!out.status.success(), "the run reported success: {said}");
    assert!(said.contains("NOT RESTORED"), "{said}");
    assert!(
        said.contains("could not be restored"),
        "the failure must propagate with its cause: {said}"
    );
    assert!(
        !said.contains("never-attempted"),
        "the run injected the next mutation on top of one it could not remove: {said}"
    );

    assert!(
        record.exists(),
        "the record was cleared after a failed restore"
    );
    assert_eq!(
        fs::read_to_string(&src).expect("read"),
        expected_mutated,
        "precondition for recovery: the file is still the mutated one the record names"
    );

    // Lift the injected failure, then recover for real.
    set_mode(&src, 0o644);
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(
        recovered.status,
        Some(0),
        "recovery failed: {}",
        recovered.stdout
    );
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// F. Mutation harness v2: tiers, deadlines, journal, and build reuse.
// ---------------------------------------------------------------------------

/// A second test target in the disposable project, so a profile has something other
/// than the killer's own target to run.
///
/// `unchecked_is_seven` is the only test anywhere that observes `unchecked()`, so the
/// `7 → 8` mutant is caught by this target and by nothing in the library.
fn with_integration_test(root: &Path) {
    fs::create_dir_all(root.join("tests")).expect("tests dir");
    fs::write(
        root.join("tests/it.rs"),
        "#[test]\nfn unchecked_is_seven() {\n    assert_eq!(fixture::unchecked(), 7);\n}\n\n\
         #[test]\nfn nothing_in_particular() {}\n",
    )
    .expect("integration test");
}

const LIB_KILLER: &str =
    r#"{ package = "fixture", lib = true, name = "tests::the_answer_is_right" }"#;
const INERT_KILLER: &str =
    r#"{ package = "fixture", test = "it", name = "nothing_in_particular" }"#;
const IT_PROFILE: &str = "\n[[profile]]\nname = \"it\"\nwhy = \"the integration target\"\n\
                          targets = [{ package = \"fixture\", test = \"it\" }]\n";

/// One mutation with killers and an optional profile.
fn tiered(
    root: &Path,
    name: &str,
    find: &str,
    replace: &str,
    killers: &[&str],
    profile: bool,
) -> PathBuf {
    let path = root.join(format!("{name}.toml"));
    let mut text = format!(
        "[[mutation]]\nname = \"{name}\"\nwhy = \"lifecycle fixture\"\nfile = \"{SRC_REL}\"\n\
         find = {}\nreplace = {}\n",
        toml_string(find),
        toml_string(replace)
    );
    if !killers.is_empty() {
        text.push_str(&format!("killers = [{}]\n", killers.join(", ")));
    }
    if profile {
        text.push_str("profile = \"it\"\n");
        if killers.is_empty() {
            text.push_str("no_killer = \"fixture\"\n");
        }
        text.push_str(IT_PROFILE);
    }
    fs::write(&path, text).expect("catalogue");
    path
}

/// The records of the one campaign under `dir`, in order.
fn journal(dir: &Path) -> Vec<serde_json::Value> {
    let keys: Vec<PathBuf> = fs::read_dir(dir)
        .expect("campaign dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("journal.jsonl").exists())
        .collect();
    assert_eq!(
        keys.len(),
        1,
        "expected one campaign under {}: {keys:?}",
        dir.display()
    );
    fs::read_to_string(keys[0].join("journal.jsonl"))
        .expect("journal")
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json"))
        .filter(|v| v["kind"] == "mutant")
        .collect()
}

fn tiers_run(r: &serde_json::Value) -> Vec<String> {
    r["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(|s| {
            format!(
                "{}:{}",
                s["tier"].as_str().unwrap_or("?"),
                s["outcome"].as_str().unwrap_or("?")
            )
        })
        .collect()
}

fn run_campaign(root: &Path, cat: &Path, campaigns: &Path, extra: &[&str]) -> Run {
    let mut args = vec![
        "mutate-run".to_owned(),
        "--root".to_owned(),
        root.display().to_string(),
        "--catalogue".to_owned(),
        cat.display().to_string(),
        "--campaign-dir".to_owned(),
        campaigns.display().to_string(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    xtask(&borrowed)
}

/// Every transition of the tier state machine, against real `cargo` runs.
///
/// One project, one mutant per campaign, each asserting the verdict, the tier that
/// decided it, exactly which steps ran, and that the tree is clean afterwards.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn each_tier_decides_only_while_the_mutant_is_alive() {
    let project = disposable_project();
    let root = project.path();
    with_integration_test(root);
    let campaigns = tempfile::tempdir().expect("campaigns");

    struct Case {
        name: &'static str,
        find: &'static str,
        replace: &'static str,
        killers: &'static [&'static str],
        profile: bool,
        status: i32,
        verdict: &'static str,
        tier: Option<&'static str>,
        steps: &'static [&'static str],
    }
    let cases = [
        // A verified killer fails: caught at tier 1, nothing else runs.
        Case {
            name: "direct",
            find: "u64 { 42 }",
            replace: "u64 { 43 }",
            killers: &[LIB_KILLER],
            profile: true,
            status: 0,
            verdict: "caught",
            tier: Some("direct"),
            steps: &["direct:fail"],
        },
        // The killer passes; the profile catches it; the suite does not run.
        Case {
            name: "focused",
            find: "u64 { 7 }",
            replace: "u64 { 8 }",
            killers: &[INERT_KILLER],
            profile: true,
            status: 0,
            verdict: "caught",
            tier: Some("focused"),
            steps: &["direct:pass", "focused:fail"],
        },
        // Killer and profile pass; only the suite sees it.
        Case {
            name: "full",
            find: "u64 { 42 }",
            replace: "u64 { 43 }",
            killers: &[INERT_KILLER],
            profile: true,
            status: 0,
            verdict: "caught",
            tier: Some("full"),
            steps: &["direct:pass", "focused:pass", "full:fail"],
        },
        // Everything passes, the suite last: the only way to SURVIVED.
        Case {
            name: "survivor",
            find: "u64 { 0 }",
            replace: "u64 { 1 }",
            killers: &[INERT_KILLER],
            profile: true,
            status: 1,
            verdict: "survived",
            tier: None,
            steps: &["direct:pass", "focused:pass", "full:pass"],
        },
        // No metadata at all: straight to the suite, as the legacy runner did.
        Case {
            name: "unannotated",
            find: "u64 { 42 }",
            replace: "u64 { 43 }",
            killers: &[],
            profile: false,
            status: 0,
            verdict: "caught",
            tier: Some("full"),
            steps: &["full:fail"],
        },
    ];
    for c in cases {
        let cat = tiered(root, c.name, c.find, c.replace, c.killers, c.profile);
        let dir = campaigns.path().join(c.name);
        let run = run_campaign(root, &cat, &dir, &[]);
        assert_eq!(run.status, Some(c.status), "{}: {}", c.name, run.stdout);
        let records = journal(&dir);
        assert_eq!(records.len(), 1, "{}", c.name);
        let r = &records[0];
        assert_eq!(r["verdict"], c.verdict, "{}: {}", c.name, run.stdout);
        assert_eq!(r["tier"].as_str(), c.tier, "{}", c.name);
        assert_eq!(tiers_run(r), c.steps, "{}: {}", c.name, run.stdout);
        assert_eq!(r["restored"], true);
        assert_eq!(r["restored_digest"], r["original_digest"]);
        assert_clean(root);
    }
}

/// A deadline at any tier is no verdict, and nothing after it runs.
#[test]
#[ignore = "drives nested cargo builds and kills them; run with --ignored"]
fn a_deadline_at_any_tier_is_no_verdict_and_leaves_nothing_running() {
    let project = disposable_project();
    let root = project.path();
    with_integration_test(root);
    let campaigns = tempfile::tempdir().expect("campaigns");
    let scratch = tempfile::tempdir().expect("scratch");

    // The slow mutant: the library test sleeps thirty seconds. Each case gives one tier
    // a three-second deadline; the others keep theirs.
    for (name, killers, profile, flag, steps) in [
        (
            "killer-times-out",
            &[LIB_KILLER][..],
            false,
            "--killer-deadline-secs",
            vec!["direct:timeout"],
        ),
        (
            "suite-times-out",
            &[][..],
            false,
            "--broad-deadline-secs",
            vec!["full:timeout"],
        ),
    ] {
        let cat = tiered(root, name, "u64 { 0 }", "u64 { 30_000 }", killers, profile);
        let dir = campaigns.path().join(name);
        let ready = scratch.path().join(format!("{name}.pid"));
        let args = vec![
            "mutate-run".to_owned(),
            "--root".to_owned(),
            root.display().to_string(),
            "--catalogue".to_owned(),
            cat.display().to_string(),
            "--campaign-dir".to_owned(),
            dir.display().to_string(),
            flag.to_owned(),
            "3".to_owned(),
        ];
        let out = Command::new(XTASK)
            .args(&args)
            .env("NAZM_FIXTURE_READY", &ready)
            .output()
            .expect("run");
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        assert_eq!(out.status.code(), Some(1), "{name}: {said}");
        assert!(said.contains("TIMEOUT"), "{name}: {said}");
        assert!(
            said.contains("neither caught nor survived"),
            "{name}: {said}"
        );
        let records = journal(&dir);
        assert_eq!(records[0]["verdict"], "timeout", "{name}");
        assert_eq!(
            tiers_run(&records[0]),
            steps,
            "{name}: a step ran after the timeout"
        );
        // The test process the deadline interrupted is gone, with its whole tree.
        let pid: u32 = fs::read_to_string(&ready)
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .expect("the mutated test announced itself");
        until(
            "the timed-out test to be gone",
            Duration::from_secs(20),
            || {
                if pid_alive(pid) {
                    Err(format!("{pid} alive"))
                } else {
                    Ok(())
                }
            },
        );
        assert_clean(root);
    }

    // A profile target that times out: the killer passes, the profile target sleeps.
    let name = "profile-times-out";
    fs::write(
        root.join("tests/it.rs"),
        "#[test]\nfn unchecked_is_seven() {\n    std::thread::sleep(std::time::Duration::from_millis(fixture::delay_ms()));\n}\n\n\
         #[test]\nfn nothing_in_particular() {}\n",
    )
    .expect("slow it");
    let cat = tiered(
        root,
        name,
        "u64 { 0 }",
        "u64 { 30_000 }",
        &[INERT_KILLER],
        true,
    );
    let dir = campaigns.path().join(name);
    let run = run_campaign(root, &cat, &dir, &["--profile-deadline-secs", "3"]);
    assert_eq!(run.status, Some(1), "{}", run.stdout);
    let records = journal(&dir);
    assert_eq!(records[0]["verdict"], "timeout");
    assert_eq!(tiers_run(&records[0]), ["direct:pass", "focused:timeout"]);
    assert_clean(root);
}

/// Stale killer metadata stops the campaign before anything is injected.
#[test]
#[ignore = "drives a nested cargo build; run with --ignored"]
fn a_killer_that_names_no_test_refuses_the_campaign_before_injection() {
    let project = disposable_project();
    let root = project.path();
    let campaigns = tempfile::tempdir().expect("campaigns");
    let cat = tiered(
        root,
        "stale",
        "u64 { 42 }",
        "u64 { 43 }",
        &[r#"{ package = "fixture", lib = true, name = "tests::renamed_away" }"#],
        false,
    );
    let run = run_campaign(root, &cat, campaigns.path(), &[]);
    assert_eq!(run.status, Some(1), "{}", run.stdout);
    assert!(run.says("is not a test in fixture --lib"), "{}", run.stdout);
    assert!(run.says("catalogue defect"), "{}", run.stdout);
    assert!(
        !run.says("caught       stale"),
        "a verdict was produced on stale metadata"
    );
    assert_clean(root);
}

/// Build reuse cannot hand one mutant another's binary.
///
/// Four builds against one target directory, in one campaign: the baseline, a mutant
/// that makes `answer()` 43, a mutant that makes it 44, and a mutant elsewhere that
/// leaves it 42. Each run of the test writes the value its binary computes. A stale
/// artefact would show up as a repeated value in the wrong place.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn every_mutant_is_measured_against_its_own_build() {
    let project = disposable_project();
    let root = project.path();
    let campaigns = tempfile::tempdir().expect("campaigns");
    let scratch = tempfile::tempdir().expect("scratch");
    let observed = scratch.path().join("observed");
    let cat = root.join("reuse.toml");
    fs::write(
        &cat,
        format!(
            "[[mutation]]\nname = \"to-43\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 43 }}\"\n\n\
             [[mutation]]\nname = \"to-44\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 44 }}\"\n\n\
             [[mutation]]\nname = \"elsewhere\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 7 }}\"\nreplace = \"u64 {{ 8 }}\"\n"
        ),
    )
    .expect("catalogue");
    for strategy in ["full", "targeted"] {
        let _ = fs::remove_file(&observed);
        let out = Command::new(XTASK)
            .args([
                "mutate-run",
                "--root",
                &root.display().to_string(),
                "--catalogue",
                &cat.display().to_string(),
                "--campaign-dir",
                &campaigns.path().join(strategy).display().to_string(),
                "--strategy",
                strategy,
            ])
            .env("NAZM_FIXTURE_OBSERVE", &observed)
            .output()
            .expect("run");
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        // `elsewhere` survives, so the run reports a problem; the point is the sequence.
        assert!(said.contains("caught       to-43"), "{strategy}: {said}");
        assert!(said.contains("caught       to-44"), "{strategy}: {said}");
        assert!(
            said.contains("SURVIVED     elsewhere"),
            "{strategy}: {said}"
        );
        let seen: Vec<String> = fs::read_to_string(&observed)
            .expect("observations")
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            seen,
            ["42", "43", "44", "42"],
            "{strategy}: a build was reused across mutants"
        );
        assert_clean(root);
    }
}

/// A campaign interrupted between mutants resumes from its journal, and only its own.
#[test]
#[ignore = "drives and kills a real mutation run; run with --ignored"]
fn an_interrupted_campaign_resumes_its_own_journal_and_refuses_any_other() {
    use std::os::unix::process::CommandExt as _;

    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let campaigns = tempfile::tempdir().expect("campaigns");
    let scratch = tempfile::tempdir().expect("scratch");
    let ready = scratch.path().join("ready.pid");
    let src = root.join(SRC_REL);
    let record = root.join("target/.nazm-mutation-backup");
    let cat = root.join("two.toml");
    fs::write(
        &cat,
        format!(
            "[[mutation]]\nname = \"quick\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 43 }}\"\n\n\
             [[mutation]]\nname = \"slow\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 0 }}\"\nreplace = \"u64 {{ 30_000 }}\"\n"
        ),
    )
    .expect("catalogue");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    let args = |resume: bool| {
        let mut v = vec![
            "mutate-run".to_owned(),
            "--root".to_owned(),
            root_s.clone(),
            "--catalogue".to_owned(),
            cat.display().to_string(),
            "--campaign-dir".to_owned(),
            campaigns.path().display().to_string(),
        ];
        if resume {
            v.push("--resume".to_owned());
        }
        v
    };

    // 1. Interrupted inside the second mutant, after the first was finalized.
    let mut runner = Command::new(XTASK)
        .args(args(false))
        .env("NAZM_FIXTURE_READY", &ready)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let pgid = runner.id();
    let descendant = wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        MUTANT_DELAY_MS,
        Duration::from_secs(240),
    );
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pgid}")])
        .status();
    runner.wait().expect("reap");
    until(
        "the watchdog to end the interrupted mutant's test",
        Duration::from_secs(30),
        || {
            if pid_alive(descendant) {
                Err(format!("{descendant} alive"))
            } else {
                Ok(())
            }
        },
    );

    // The journal holds the first and not the second: nothing is final before it is
    // restored, and the second never was.
    let before = journal(campaigns.path());
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!(before[0]["name"], "quick");

    // 2. Recovery precedes resume. Without it, the resume refuses on the stranded record.
    let refused = xtask(&args(true).iter().map(String::as_str).collect::<Vec<_>>());
    assert_ne!(refused.status, Some(0));
    assert!(
        refused.says("recovery is not automatic"),
        "{}",
        refused.stdout
    );
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(recovered.status, Some(0), "{}", recovered.stdout);

    // 3. Resumed: `quick` is reused and says so, `slow` runs. `slow` survives (nothing
    //    asserts the delay), so the run reports that.
    let resumed = xtask(&args(true).iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        resumed.says("caught       quick  (reused from journal"),
        "{}",
        resumed.stdout
    );
    assert!(resumed.says("SURVIVED     slow"), "{}", resumed.stdout);
    assert!(
        resumed.says("of which reused from journal 1"),
        "{}",
        resumed.stdout
    );
    assert_clean(root);

    // 4. A different catalogue is a different campaign: nothing is reused.
    let text = fs::read_to_string(&cat).expect("read");
    fs::write(&cat, text.replace("why = \"f\"", "why = \"changed\"")).expect("edit");
    let other = xtask(&args(true).iter().map(String::as_str).collect::<Vec<_>>());
    assert!(other.says("Nothing is reused"), "{}", other.stdout);
    assert!(!other.says("(reused from journal"), "{}", other.stdout);
    assert!(
        other.says("of which reused from journal 0"),
        "{}",
        other.stdout
    );
    assert!(
        other.says("caught       quick  (tier 3"),
        "{}",
        other.stdout
    );
    assert_clean(root);

    // 5. So is a different tree.
    fs::write(&cat, &text).expect("restore catalogue");
    fs::write(root.join("unrelated.txt"), "a change to the tree").expect("tree");
    let tree = xtask(&args(true).iter().map(String::as_str).collect::<Vec<_>>());
    assert!(tree.says("Nothing is reused"), "{}", tree.stdout);
    assert!(!tree.says("(reused from journal"), "{}", tree.stdout);
    assert!(
        tree.says("of which reused from journal 0"),
        "{}",
        tree.stdout
    );
    assert_clean(root);

    // 6. So is a different execution profile (N14.1): a verdict reached with four test
    //    threads is not evidence for a campaign run with one. The same profile, resumed,
    //    reuses everything, so the refusal is the profile's doing and nothing else's.
    let same = xtask(&args(true).iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        same.says("of which reused from journal 2"),
        "{}",
        same.stdout
    );
    let mut threads = args(true);
    threads.extend(["--test-threads".to_owned(), "4".to_owned()]);
    let profile = xtask(&threads.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(profile.says("Nothing is reused"), "{}", profile.stdout);
    assert!(profile.says("execution profile"), "{}", profile.stdout);
    assert!(!profile.says("(reused from journal"), "{}", profile.stdout);
    assert!(
        profile.says("of which reused from journal 0"),
        "{}",
        profile.stdout
    );
    assert_clean(root);
}

/// The watchdog: when the harness dies, its invocation's tree goes with it.
///
/// Two ways to die: SIGKILL, which nothing can intercept, and SIGINT, the terminal's
/// ^C, which the harness does not handle and which does not reach the invocation's own
/// group. In both, a grandchild that the harness never learned about must stop.
#[test]
#[ignore = "kills the harness mid-invocation; run with --ignored"]
fn the_invocation_dies_with_the_harness_however_the_harness_dies() {
    use std::os::unix::process::CommandExt as _;
    for signal in ["-KILL", "-INT"] {
        let scratch = tempfile::tempdir().expect("scratch");
        let gc = scratch.path().join("gc.pid");
        let mut harness = Command::new(XTASK)
            .args([
                "mutate-exec",
                "--script",
                &format!("sleep 120 & echo $! > {}; wait", gc.display()),
                "--deadline-secs",
                "300",
            ])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn harness");
        let mut pid = 0u32;
        until(
            "the grandchild to start",
            Duration::from_secs(30),
            || match fs::read_to_string(&gc)
                .ok()
                .and_then(|s| s.trim().parse().ok())
            {
                Some(p) => {
                    pid = p;
                    Ok(())
                }
                None => Err("not yet".into()),
            },
        );
        let _ = Command::new("kill")
            .args([signal, &harness.id().to_string()])
            .status();
        harness.wait().expect("reap harness");
        until(
            &format!("the grandchild to stop after {signal}"),
            Duration::from_secs(30),
            || {
                if pid_alive(pid) {
                    Err(format!("{pid} alive"))
                } else {
                    Ok(())
                }
            },
        );
    }
}

/// SIGINT during a real mutant: the tree is recoverable and nothing was finalized.
#[test]
#[ignore = "interrupts a real mutation run; run with --ignored"]
fn a_sigint_during_a_mutant_leaves_it_unfinalized_and_recoverable() {
    use std::os::unix::process::CommandExt as _;
    let project = disposable_project();
    let root = project.path();
    let root_s = root.display().to_string();
    let campaigns = tempfile::tempdir().expect("campaigns");
    let scratch = tempfile::tempdir().expect("scratch");
    let ready = scratch.path().join("ready.pid");
    let src = root.join(SRC_REL);
    let record = root.join("target/.nazm-mutation-backup");
    let cat = tiered(
        root,
        "slow",
        "u64 { 0 }",
        "u64 { 30_000 }",
        &[LIB_KILLER],
        false,
    );
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);

    let mut runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
            "--campaign-dir",
            &campaigns.path().display().to_string(),
        ])
        .env("NAZM_FIXTURE_READY", &ready)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let descendant = wait_until_mutated(
        &src,
        &record,
        &ready,
        &expected_mutated,
        MUTANT_DELAY_MS,
        Duration::from_secs(240),
    );
    // The terminal's ^C, delivered to the runner's group: the runner and its watchdog's
    // group are separate, so this reaches the runner only.
    let _ = Command::new("kill")
        .args(["-INT", "--", &format!("-{}", runner.id())])
        .status();
    runner.wait().expect("reap");
    until(
        "the watchdog to end the mutant's test",
        Duration::from_secs(30),
        || {
            if pid_alive(descendant) {
                Err(format!("{descendant} alive"))
            } else {
                Ok(())
            }
        },
    );
    // Unfinalized: the journal has its header and no record.
    let key_dirs: Vec<PathBuf> = fs::read_dir(campaigns.path())
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    for d in key_dirs {
        let text = fs::read_to_string(d.join("journal.jsonl")).unwrap_or_default();
        assert!(
            !text.contains("\"kind\":\"mutant\""),
            "a mutant was finalized by an interrupted run: {text}"
        );
    }
    // Recoverable, by the attested procedure.
    assert!(record.exists());
    let recovered = xtask(&["mutate-recover", "--root", &root_s, "--confirm-no-writers"]);
    assert_eq!(recovered.status, Some(0), "{}", recovered.stdout);
    assert!(
        recovered.says(&format!("repaired {SRC_REL}")),
        "{}",
        recovered.stdout
    );
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// N32-H. One warm session verifies every killer around its mutant's one injection.
// ---------------------------------------------------------------------------

/// Mutant A, then B, then A again, in one session and one target directory, with no
/// `cargo clean` between them: each killer passes pristine, fails on its mutant and passes
/// restored, and every build the killer ran was of the bytes it was run for. A stale
/// artefact — a mutant's executable reused after restoration, or the pristine one reused
/// under a mutant — would show here as a missing or out-of-order observation.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn a_warm_session_verifies_every_killer_against_each_mutants_own_bytes() {
    let project = disposable_project();
    let root = project.path();
    with_integration_test(root);
    let campaigns = tempfile::tempdir().expect("campaigns");
    let scratch = tempfile::tempdir().expect("scratch");
    let observed = scratch.path().join("observed");
    let cat = root.join("warm.toml");
    fs::write(
        &cat,
        format!(
            "[[mutation]]\nname = \"a\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 43 }}\"\nkillers = [{LIB_KILLER}]\n\n\
             [[mutation]]\nname = \"b\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 7 }}\"\nreplace = \"u64 {{ 8 }}\"\nkillers = [{{ package = \"fixture\", test = \"it\", name = \"unchecked_is_seven\" }}]\n\n\
             [[mutation]]\nname = \"a-again\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ 44 }}\"\nkillers = [{LIB_KILLER}]\n"
        ),
    )
    .expect("catalogue");
    let out = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root.display().to_string(),
            "--catalogue",
            &cat.display().to_string(),
            "--campaign-dir",
            &campaigns.path().display().to_string(),
            "--strategy",
            "targeted",
            "--verify-killers",
        ])
        .env("NAZM_FIXTURE_OBSERVE", &observed)
        .output()
        .expect("run");
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{said}");
    for name in ["a", "b", "a-again"] {
        assert!(
            said.contains(&format!("caught       {name}  ")),
            "{name}: {said}"
        );
    }
    assert!(
        said.contains("killer verification  3 of 3 verified in this session"),
        "{said}"
    );
    assert_eq!(said.matches("VERIFIED").count(), 3, "{said}");
    // One setup for the session, whatever the number of mutants.
    assert_eq!(
        said.matches("session timing       setup").count(),
        1,
        "{said}"
    );
    assert!(said.contains("3 mutant(s) decided"), "{said}");
    // Every lib-test run, in order: the baseline suite, then for `a` pristine, its verdict
    // and its verification on the mutant, restored; `b` runs another target; then the
    // same for `a-again`, whose mutant must be 44 — not a leftover 43 — and whose
    // pristine run must be 42 again.
    let seen: Vec<String> = fs::read_to_string(&observed)
        .expect("observations")
        .lines()
        .map(str::to_owned)
        .collect();
    // N32-H2: the library killer runs once on the pristine tree for both mutants that
    // declare it, and each mutant's verdict run is the verification's mutant run.
    assert_eq!(
        seen,
        ["42", "43", "42", "44", "42"],
        "a build was reused across a mutant or its restoration: {said}"
    );
    assert!(
        said.contains("mutation baseline    2 unique killer(s)"),
        "{said}"
    );
    // Nothing was deferred, so the workspace suite never ran.
    assert!(!said.contains("baseline suite"), "{said}");
    let records = journal(campaigns.path());
    assert_eq!(records.len(), 3);
    for r in &records {
        assert_eq!(r["verdict"], "caught");
        assert_eq!(r["restored_digest"], r["original_digest"]);
    }
    assert_clean(root);
}

/// A resumed session reuses journaled verdicts but cannot have verified their killers,
/// so it says so and is incomplete rather than clean.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn a_resumed_session_does_not_claim_killers_it_did_not_verify() {
    let project = disposable_project();
    let root = project.path();
    let campaigns = tempfile::tempdir().expect("campaigns");
    let cat = tiered(
        root,
        "direct",
        "u64 { 42 }",
        "u64 { 43 }",
        &[LIB_KILLER],
        false,
    );
    let first = run_campaign(root, &cat, campaigns.path(), &["--strategy", "targeted"]);
    assert_eq!(first.status, Some(0), "{}", first.stdout);
    let again = run_campaign(
        root,
        &cat,
        campaigns.path(),
        &["--strategy", "targeted", "--resume", "--verify-killers"],
    );
    assert_eq!(again.status, Some(3), "{}", again.stdout);
    assert!(
        again.says("reused, not verified here: direct"),
        "{}",
        again.stdout
    );
    assert!(
        again.says("0 of 0 verified in this session"),
        "{}",
        again.stdout
    );
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// N32-H2. The mutation baseline, and a persistent target across sessions.
// ---------------------------------------------------------------------------

/// A session in `root` with `target` as its Cargo target, observing every library-test run.
fn session(
    root: &Path,
    cat: &Path,
    campaigns: &Path,
    target: &Path,
    observed: &Path,
    extra: &[&str],
) -> Run {
    let mut args = vec![
        "mutate-run".to_owned(),
        "--root".to_owned(),
        root.display().to_string(),
        "--catalogue".to_owned(),
        cat.display().to_string(),
        "--campaign-dir".to_owned(),
        campaigns.display().to_string(),
        "--strategy".to_owned(),
        "targeted".to_owned(),
        "--verify-killers".to_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let out = Command::new(XTASK)
        .args(&args)
        .env("CARGO_TARGET_DIR", target)
        .env("NAZM_FIXTURE_OBSERVE", observed)
        .output()
        .expect("run");
    Run {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr),
    }
}

/// The project, written afresh at `root` — every file new, as a contained session's copy
/// is extracted with fresh modification times — whatever was there before.
fn fresh_copy(template: &Path, root: &Path) {
    let _ = fs::remove_dir_all(root);
    for rel in ["Cargo.toml", "Cargo.lock", SRC_REL, "tests/it.rs"] {
        let to = root.join(rel);
        fs::create_dir_all(to.parent().expect("parent")).expect("dirs");
        fs::write(&to, fs::read(template.join(rel)).expect("template")).expect("copy");
    }
}

fn one_mutation(root: &Path, name: &str, replace: &str) -> PathBuf {
    let cat = root.join(format!("{name}.toml"));
    fs::write(
        &cat,
        format!(
            "[[mutation]]\nname = \"{name}\"\nwhy = \"f\"\nfile = \"{SRC_REL}\"\nfind = \"u64 {{ 42 }}\"\nreplace = \"u64 {{ {replace} }}\"\nkillers = [{LIB_KILLER}]\n"
        ),
    )
    .expect("catalogue");
    cat
}

fn verdicts(dir: &Path) -> Vec<(String, String, String)> {
    journal(dir)
        .iter()
        .map(|r| {
            (
                r["verdict"].as_str().unwrap_or("?").to_owned(),
                r["tier"].as_str().unwrap_or("?").to_owned(),
                format!("{:?}", tiers_run(r)),
            )
        })
        .collect()
}

/// Two sessions, each a fresh copy at the same path, one persistent target between them:
/// the bytes every run executed are exactly each session's own — 42, 43, 42, then 42, 44,
/// 42 — and never a mutant or a pristine build left by the other. A poisoned target is
/// rebuilt, a warm target decides exactly what a cold one does, and deleting the target
/// changes no verdict a resume reuses.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn a_fresh_copy_against_a_persistent_target_measures_only_its_own_bytes() {
    let template = disposable_project();
    with_integration_test(template.path());
    let work = tempfile::tempdir().expect("work");
    let root = work.path().join("copy");
    let target = work.path().join("persistent-target");
    let observed = work.path().join("observed");
    let cats = tempfile::tempdir().expect("catalogues");
    let to43 = one_mutation(cats.path(), "to-43", "43");
    let to44 = one_mutation(cats.path(), "to-44", "44");

    fresh_copy(template.path(), &root);
    let one = session(
        &root,
        &to43,
        &work.path().join("c1"),
        &target,
        &observed,
        &[],
    );
    assert_eq!(one.status, Some(0), "{}", one.stdout);
    assert_clean(&root);
    fresh_copy(template.path(), &root);
    let two = session(
        &root,
        &to44,
        &work.path().join("c2"),
        &target,
        &observed,
        &[],
    );
    assert_eq!(two.status, Some(0), "{}", two.stdout);
    assert_clean(&root);
    let seen: Vec<String> = fs::read_to_string(&observed)
        .expect("observations")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        seen,
        ["42", "43", "42", "42", "44", "42"],
        "a stale build crossed sessions"
    );

    // A poisoned target: every fixture artefact overwritten. Where rustc's incremental
    // cache would reuse the damaged object files — the source it hashed is unchanged — the
    // pristine build fails and a session whose target is the harness's own cleans it and
    // builds once more; where Cargo rebuilds them itself, nothing more is needed. Either
    // way the verdict is the verdict.
    for dir in [target.join("debug/deps"), target.join("debug")] {
        for e in fs::read_dir(&dir).expect("target").flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let fixture =
                n.starts_with("libfixture") || n.starts_with("fixture-") || n.starts_with("it-");
            if fixture && e.path().is_file() {
                fs::write(e.path(), b"poison").expect("poison");
            }
        }
    }
    let _ = fs::remove_file(&observed);
    fresh_copy(template.path(), &root);
    let poisoned = session(
        &root,
        &to43,
        &work.path().join("c3"),
        &target,
        &observed,
        &["--disposable-target"],
    );
    // Either route is correct, and which one a platform takes is Cargo's and rustc's:
    // on macOS rustc's incremental cache reuses the damaged objects and the build fails, so
    // the harness cleans the target and builds once more; on Linux Cargo rebuilds them by
    // itself. What must hold on both is the verdict and the bytes each run executed.
    assert_eq!(poisoned.status, Some(0), "{}", poisoned.stdout);
    assert!(poisoned.says("caught       to-43"), "{}", poisoned.stdout);
    assert_eq!(
        fs::read_to_string(&observed).expect("observations"),
        "42\n43\n42\n",
        "a poisoned target decided a verdict"
    );
    // Warm and cold decide identically: the same campaign against the warm target, and
    // against a target that has never been built.
    assert_eq!(
        verdicts(&work.path().join("c3")),
        verdicts(&work.path().join("c1"))
    );
    let cold = work.path().join("cold-target");
    fresh_copy(template.path(), &root);
    let c = session(&root, &to43, &work.path().join("c4"), &cold, &observed, &[]);
    assert_eq!(c.status, Some(0), "{}", c.stdout);
    assert_eq!(
        verdicts(&work.path().join("c4")),
        verdicts(&work.path().join("c1"))
    );
    // Deleting the cache changes nothing a resume reuses.
    fs::remove_dir_all(&target).expect("delete the target");
    fresh_copy(template.path(), &root);
    let resumed = session(
        &root,
        &to43,
        &work.path().join("c1"),
        &target,
        &observed,
        &["--resume"],
    );
    assert!(resumed.says("reused from journal"), "{}", resumed.stdout);
    assert_eq!(verdicts(&work.path().join("c1")).len(), 1);
    assert_clean(&root);
}

/// Case A: a test nobody declared fails on the pristine tree. Tier-1 verdicts need only
/// their own killers to pass without the mutant, so the session decides and says nothing
/// about the workspace — whose health is the MILESTONE gate's workspace suite to establish,
/// and which that suite would refuse.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn an_unrelated_failing_test_is_not_the_mutation_baselines_to_judge() {
    let project = disposable_project();
    let root = project.path();
    fs::create_dir_all(root.join("tests")).expect("tests dir");
    fs::write(
        root.join("tests/it.rs"),
        "#[test]\nfn unchecked_is_seven() {\n    assert_eq!(fixture::unchecked(), 7);\n}\n\n\
         #[test]\nfn unrelated_is_broken() {\n    panic!(\"broken on purpose\");\n}\n",
    )
    .expect("integration test");
    let campaigns = tempfile::tempdir().expect("campaigns");
    let cat = tiered(
        root,
        "direct",
        "u64 { 42 }",
        "u64 { 43 }",
        &[LIB_KILLER],
        false,
    );
    let run = run_campaign(
        root,
        &cat,
        campaigns.path(),
        &["--strategy", "targeted", "--verify-killers"],
    );
    assert_eq!(run.status, Some(0), "{}", run.stdout);
    assert!(run.says("caught       direct"), "{}", run.stdout);
    assert!(run.says("1 of 1 verified"), "{}", run.stdout);
    assert!(!run.says("baseline suite"), "{}", run.stdout);
    assert!(!run.says("workspace is"), "{}", run.stdout);
    // The workspace suite itself is red, as the MILESTONE gate's own run would find.
    let suite = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(root)
        .output()
        .expect("cargo test");
    assert!(!suite.status.success());
    assert_clean(root);
}

/// Case B: a declared killer that fails without any mutant cannot catch one. The session
/// refuses before injecting anything.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn a_killer_that_fails_on_the_pristine_tree_refuses_the_session() {
    let project = disposable_project();
    let root = project.path();
    fs::create_dir_all(root.join("tests")).expect("tests dir");
    fs::write(
        root.join("tests/it.rs"),
        "#[test]\nfn already_failing() {\n    panic!(\"fails without a mutant\");\n}\n",
    )
    .expect("integration test");
    let campaigns = tempfile::tempdir().expect("campaigns");
    let cat = tiered(
        root,
        "red",
        "u64 { 42 }",
        "u64 { 43 }",
        &[r#"{ package = "fixture", test = "it", name = "already_failing" }"#],
        false,
    );
    let run = run_campaign(
        root,
        &cat,
        campaigns.path(),
        &["--strategy", "targeted", "--verify-killers"],
    );
    assert_eq!(run.status, Some(1), "{}", run.stdout);
    assert!(
        run.says("do not pass on the pristine tree"),
        "{}",
        run.stdout
    );
    assert!(!run.says("caught       red"), "{}", run.stdout);
    assert_clean(root);
}

// ---------------------------------------------------------------------------
// N32-H3. Bucketed execution across packages, one mutation target.
// ---------------------------------------------------------------------------

/// A two-package workspace, `a` and `b`, where `b` depends on `a` — written afresh at
/// `root`. Each library test records the value its binary computed.
fn two_packages(root: &Path) {
    let _ = fs::remove_dir_all(root);
    let file = |rel: &str, text: &str| {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().expect("parent")).expect("dirs");
        fs::write(p, text).expect("write");
    };
    file(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
    );
    file(
        "Cargo.lock",
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\n\
         version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n\
         [[package]]\nname = \"b\"\nversion = \"0.0.0\"\ndependencies = [\n \"a\",\n]\n",
    );
    file(
        "a/Cargo.toml",
        "[package]\nname = \"a\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    file("b/Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\na = { path = \"../a\" }\n");
    let observed = |name: &str, expr: &str, want: u64| {
        format!(
            "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn it_is_right() {{\n        \
             if let Ok(p) = std::env::var(\"NAZM_FIXTURE_OBSERVE\") {{\n            use std::io::Write as _;\n            \
             if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {{\n                \
             let _ = writeln!(f, \"{name}{{}}\", {expr});\n            }}\n        }}\n        \
             assert_eq!({expr}, {want});\n    }}\n}}\n"
        )
    };
    file(
        "a/src/lib.rs",
        &format!(
            "pub fn value() -> u64 {{ 42 }}\n\n{}",
            observed("a", "super::value()", 42)
        ),
    );
    file(
        "b/src/lib.rs",
        &format!(
            "pub fn value() -> u64 {{ a::value() + 100 }}\n\n{}",
            observed("b", "super::value()", 142)
        ),
    );
}

fn in_package(
    dir: &Path,
    name: &str,
    file: &str,
    find: &str,
    replace: &str,
    package: &str,
) -> PathBuf {
    let cat = dir.join(format!("{name}.toml"));
    fs::write(
        &cat,
        format!(
            "[[mutation]]\nname = \"{name}\"\nwhy = \"f\"\nfile = \"{file}\"\nfind = {}\nreplace = {}\n\
             killers = [{{ package = \"{package}\", lib = true, name = \"tests::it_is_right\" }}]\n",
            toml_string(find),
            toml_string(replace)
        ),
    )
    .expect("catalogue");
    cat
}

/// Package `a`'s mutant, then `b`'s, then — in the next session, against the same
/// persistent mutation target — `a`'s again: every killer run executed exactly its own
/// bytes. `b` depends on `a`, so a stale `a` from the first mutant would show as `b143`.
#[test]
#[ignore = "drives nested cargo builds; run with --ignored"]
fn package_a_then_b_then_a_share_one_target_without_a_stale_artefact() {
    let work = tempfile::tempdir().expect("work");
    let root = work.path().join("copy");
    let target = work.path().join("mutation-target");
    let observed = work.path().join("observed");
    let cats = tempfile::tempdir().expect("catalogues");
    let two = cats.path().join("a-then-b.toml");
    let a1 = fs::read_to_string(in_package(
        cats.path(),
        "a-to-43",
        "a/src/lib.rs",
        "u64 { 42 }",
        "u64 { 43 }",
        "a",
    ))
    .expect("a");
    let b1 = fs::read_to_string(in_package(
        cats.path(),
        "b-plus-200",
        "b/src/lib.rs",
        "a::value() + 100",
        "a::value() + 200",
        "b",
    ))
    .expect("b");
    fs::write(&two, format!("{b1}\n{a1}")).expect("catalogue");
    let a2 = in_package(
        cats.path(),
        "a-to-44",
        "a/src/lib.rs",
        "u64 { 42 }",
        "u64 { 44 }",
        "a",
    );

    two_packages(&root);
    let one = session(
        &root,
        &two,
        &work.path().join("c1"),
        &target,
        &observed,
        &[],
    );
    assert_eq!(one.status, Some(0), "{}", one.stdout);
    // Bucketed: package `a` first although the catalogue names `b` first.
    let a_at = one.stdout.find("caught       a-to-43").expect("a decided");
    let b_at = one
        .stdout
        .find("caught       b-plus-200")
        .expect("b decided");
    assert!(a_at < b_at, "{}", one.stdout);
    two_packages(&root);
    let two_run = session(&root, &a2, &work.path().join("c2"), &target, &observed, &[]);
    assert_eq!(two_run.status, Some(0), "{}", two_run.stdout);
    let seen: Vec<String> = fs::read_to_string(&observed)
        .expect("observations")
        .lines()
        .map(str::to_owned)
        .collect();
    // Session 1: the baseline runs a and b once each, then a's mutant and its restoration,
    // then b's mutant — built against the restored a — and its restoration. Session 2:
    // a's baseline, its mutant, its restoration.
    assert_eq!(
        seen,
        ["a42", "b142", "a43", "a42", "b242", "b142", "a42", "a44", "a42"],
        "a stale artefact crossed a package or a session"
    );
}
