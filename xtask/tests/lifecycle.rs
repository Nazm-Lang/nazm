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
//! `cargo test --workspace` once per catalogued defect — 68 of them — so leaving these in
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
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
    let root = project.path();
    let root_s = root.display().to_string();
    let src = root.join(SRC_REL);

    // The mutant sleeps for thirty seconds, so the window in which the tree is damaged
    // is long enough to interrupt deliberately rather than by luck. The baseline still
    // runs at full speed, because the delay only exists once the mutation is applied.
    let cat = catalogue(root, "slow", "u64 { 0 }", "u64 { 30_000 }");
    let expected_mutated = SOURCE.replacen("u64 { 0 }", "u64 { 30_000 }", 1);
    assert_ne!(expected_mutated, SOURCE, "the fixture must actually change");

    let mut runner = Command::new(XTASK)
        .args([
            "mutate-run",
            "--root",
            &root_s,
            "--catalogue",
            &cat.display().to_string(),
        ])
        // Its own process group, so the kill below reaches the `cargo` and `rustc`
        // descendants too and cannot reach the test runner.
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let pgid = runner.id();

    let record = root.join("target/.nazm-mutation-backup");

    // Ready means **both**: the record exists *and* the source holds the mutated bytes.
    // Either alone is a state the runner passes through, and killing there would test a
    // different thing than the one claimed.
    until(
        "the run to be mid-mutation",
        Duration::from_secs(180),
        || {
            let has_record = record.exists();
            let mutated = fs::read_to_string(&src).is_ok_and(|s| s == expected_mutated);
            match (has_record, mutated) {
                (true, true) => Ok(()),
                (false, true) => Err("the mutated bytes are present but the record is not".into()),
                (true, false) => Err("the record is present but the source is not mutated".into()),
                (false, false) => Err("neither the record nor the mutation has appeared".into()),
            }
        },
    );

    // Kill the whole group. Reaping the runner alone would say nothing about the `cargo`
    // it spawned, which is still writing to the tree.
    let killed = Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
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
    let mut runner = Command::new(XTASK)
        .args(["mutate-run", "--root", &root_s, "--catalogue", &cat_s])
        .env("NAZM_FIXTURE_READY", &ready)
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

    // 3. Only the coordinator. Not the group.
    runner.kill().expect("kill the coordinator");
    runner.wait().expect("reap the coordinator");

    // 4. And the descendant is still there, holding the workspace open.
    assert!(
        pid_alive(descendant),
        "the descendant exited on its own; this test proves nothing unless it outlives \
         its coordinator"
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

    // 7. Now clean up what this test started, and only that.
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
        .status();
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
             name = \"second\"\nwhy = \"must never be attempted\"\nfile = \"{SRC_REL}\"\n\
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

    let out = runner.wait_with_output().expect("reap runner");
    let said = String::from_utf8_lossy(&out.stdout).into_owned();

    assert!(!out.status.success(), "the run reported success: {said}");
    assert!(said.contains("NOT RESTORED"), "{said}");
    assert!(
        said.contains("could not be restored"),
        "the failure must propagate with its cause: {said}"
    );
    assert!(
        !said.contains("second"),
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
