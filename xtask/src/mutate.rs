//! Automated mutation verification.
//!
//! Injects each catalogued defect, rebuilds, and requires the test suite to fail — then
//! restores and requires it to pass.
//!
//! The important part is not the injection; it is proving the injection **landed**. Twice
//! during development a hand-run replacement matched nothing because `cargo fmt` had
//! rewrapped the target, the suite passed, and that outcome was indistinguishable from a
//! surviving mutation. A mutation run that cannot tell "my test is weak" from "my edit did
//! nothing" produces false confidence in whichever direction is more comfortable.
//!
//! So every injection is checked three ways: the pattern must occur exactly once, the file
//! must differ afterwards, and the mutated tree must still compile. A mutation that does
//! not build is reported as unusable rather than counted as caught — an uncompilable
//! program fails the suite for the wrong reason.
//!
//! **"Compiles" has to include the tests.** An earlier version built only
//! `cargo build --workspace`, which leaves test targets unbuilt. A broken test file then
//! made `cargo test` produce no results at all, `failing_tests` counted zero, and every
//! single mutation reported SURVIVED — nineteen of them, all false. The same class of
//! mistake as the silently-skipped injection, and just as convincing. The baseline is now
//! checked before any mutation runs, and the mutated tree is built with `--tests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where a file's original contents live while it is mutated.
///
/// The runner restores the file after each mutation — but only if it lives to do so. A
/// killed process leaves the mutation applied, and nothing notices: the next `cargo test`
/// fails for a reason that looks like a real regression. That happened during development
/// (a backgrounded run was killed mid-mutation and left `unverifiable-cleanup-passes`
/// applied), which is the same failure class the harness under test exists to prevent:
/// state written by a process that did not survive to clean it up.
///
/// So the original is written to disk *before* the mutation is applied, and a stranded
/// backup is detected and restored at startup. The write-ahead pattern the ledger uses,
/// for the same reason.
fn backup_path(root: &Path) -> PathBuf {
    root.join("target").join(".nazm-mutation-backup")
}

fn lock_path(root: &Path) -> PathBuf {
    root.join("target").join(".nazm-mutation-lock")
}

/// Refuse to run while another mutation run holds the tree.
///
/// Two runs at once corrupt each other's restores. Worse — and this is how the need was
/// found — a mutation run in the background while an ordinary `cargo test` runs in the
/// foreground makes the test suite fail against a mutated tree, and those failures look
/// exactly like real regressions. Several minutes were spent diagnosing "broken" tests
/// that were only ever reading a file another process had deliberately damaged.
///
/// Held as a file containing the pid, removed on completion. A stale lock from a killed
/// run is detected by signalling the pid.
struct RunLock {
    path: PathBuf,
}

impl RunLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let path = lock_path(root);
        if let Ok(existing) = fs::read_to_string(&path) {
            let pid = existing.trim();
            if pid_is_alive(pid) {
                return Err(format!(
                    "another mutation run is in progress (pid {pid}). Two runs corrupt \
                     each other's restores, and a concurrent `cargo test` would see a \
                     mutated tree and report failures that are not real."
                ));
            }
            // The holder is gone; `restore_stranded` deals with whatever it left.
            let _ = fs::remove_file(&path);
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // `create_new` is the race-free part: read-then-write lets two runs both see no
        // lock and both create one. The pid check above only handles a *stale* lock.
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("could not take the mutation lock: {e}"))?;
        use std::io::Write as _;
        write!(f, "{}", std::process::id())
            .map_err(|e| format!("could not record the mutation lock holder: {e}"))?;
        Ok(Self { path })
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn pid_is_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// One catalogued defect. Every field is required: a mutation that cannot say what it
/// breaks, or why, is not reviewable.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Mutation {
    pub(crate) name: String,
    pub(crate) why: String,
    pub(crate) file: String,
    pub(crate) find: String,
    pub(crate) replace: String,
}

#[derive(Debug, serde::Deserialize)]
struct Catalogue {
    mutation: Vec<Mutation>,
}

pub(crate) enum Verdict {
    /// Injected, compiled, the suite ran to completion, and it failed. Covered.
    Caught { failing: usize },
    /// Injected, compiled, the suite ran to completion, and everything passed. A gap.
    Survived,
    /// The pattern was absent, or matched more than once. Not a result about the tests.
    NotInjected(String),
    /// The mutated tree does not compile, so the suite would fail for the wrong reason.
    DidNotBuild,
    /// The suite did not run to completion: the runner errored, produced no results, or
    /// produced fewer result lines than the baseline.
    ///
    /// Neither caught nor survived. A run that did not happen tells you nothing, and the
    /// two ways of pretending otherwise are both wrong: calling it `Survived` invents a
    /// coverage gap, calling it `Caught` invents coverage. This variant exists because
    /// the runner already made the second mistake once, at scale — a test file that
    /// failed to compile produced zero results and nineteen false SURVIVEDs.
    RunnerFailed(String),
}

pub(crate) fn load(path: &Path) -> Result<Vec<Mutation>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let catalogue: Catalogue =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if catalogue.mutation.is_empty() {
        return Err("catalogue has no [[mutation]] entries".to_owned());
    }
    Ok(catalogue.mutation)
}

pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>> {
    let mutations = match load(catalogue) {
        Ok(m) => m,
        Err(e) => return Err(vec![e]),
    };

    let _lock = match RunLock::acquire(root) {
        Ok(l) => l,
        Err(e) => return Err(vec![e]),
    };

    // A previous run may have been killed mid-mutation. Repair before measuring anything,
    // and say so: a silently repaired tree leaves whoever saw the last run's output
    // wondering why it looked wrong.
    match restore_stranded(root) {
        Ok(Some(file)) => {
            println!("  restored     {file} (a previous run was killed mid-mutation)");
        }
        Ok(None) => {}
        Err(e) => return Err(vec![e]),
    }

    // Establish a baseline before touching anything. If the suite is already failing, or
    // does not run, every result below is meaningless.
    if !builds(root) {
        return Err(vec![
            "the unmutated workspace does not build (including tests). Fix that first: \
             every mutation would otherwise be reported as SURVIVED."
                .to_owned(),
        ]);
    }
    let baseline_results = match run_tests(root) {
        TestRun::Failed(why) => {
            return Err(vec![format!(
                "the baseline suite did not run to completion ({why}). Every verdict \
                 below would be meaningless."
            )])
        }
        TestRun::Completed { failing, .. } if failing > 0 => {
            return Err(vec![format!(
                "{failing} test(s) already fail before any mutation. A mutation cannot be \
                 shown to be caught by a suite that is not green."
            )])
        }
        TestRun::Completed { result_lines, .. } => result_lines,
    };

    let mut problems = Vec::new();
    for m in &mutations {
        match apply_and_test(root, m, baseline_results) {
            Verdict::Caught { failing } => {
                println!("  caught       {}  ({failing} test(s) failed)", m.name);
            }
            Verdict::Survived => {
                println!("  SURVIVED     {}", m.name);
                problems.push(format!(
                    "{}: injected and compiled, but the suite still passed. {}",
                    m.name, m.why
                ));
            }
            Verdict::NotInjected(reason) => {
                println!("  NOT INJECTED {}", m.name);
                problems.push(format!(
                    "{}: {reason}. A mutation that does not apply proves nothing — fix the \
                     pattern rather than reading the pass as coverage.",
                    m.name
                ));
            }
            Verdict::DidNotBuild => {
                println!("  UNUSABLE     {}", m.name);
                problems.push(format!(
                    "{}: the mutated tree does not compile, so a failing suite would not \
                     mean the defect was detected.",
                    m.name
                ));
            }
            Verdict::RunnerFailed(why) => {
                println!("  RUNNER ERROR {}", m.name);
                problems.push(format!(
                    "{}: the suite did not run to completion ({why}). This is neither \
                     caught nor survived — a run that did not happen says nothing about \
                     coverage.",
                    m.name
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn apply_and_test(root: &Path, m: &Mutation, baseline_results: usize) -> Verdict {
    let path: PathBuf = root.join(&m.file);
    let Ok(original) = fs::read_to_string(&path) else {
        return Verdict::NotInjected(format!("cannot read {}", m.file));
    };

    let occurrences = original.matches(&m.find).count();
    if occurrences == 0 {
        return Verdict::NotInjected(format!("pattern not found in {}", m.file));
    }
    if occurrences > 1 {
        return Verdict::NotInjected(format!(
            "pattern occurs {occurrences} times in {}; it must be unique",
            m.file
        ));
    }

    let mutated = original.replacen(&m.find, &m.replace, 1);
    if mutated == original {
        return Verdict::NotInjected("replacement produced an identical file".to_owned());
    }

    // Write-ahead: the original goes to disk before the file is touched, so a killed
    // process leaves a recoverable record rather than a silently mutated tree.
    //
    // The record carries the hash of the *mutated* form too. Restoring blindly would
    // overwrite whatever is on disk — including edits made after the crash, by someone
    // who had no idea a mutation was stranded there.
    let backup = backup_path(root);
    let record = format!("{}\n{}\n{original}", m.file, digest(&mutated));
    if let Some(parent) = backup.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write(&backup, &record).is_err() {
        return Verdict::NotInjected(format!("cannot write the backup for {}", m.file));
    }

    if fs::write(&path, &mutated).is_err() {
        let _ = fs::remove_file(&backup);
        return Verdict::NotInjected(format!("cannot write {}", m.file));
    }

    let verdict = if builds(root) {
        match run_tests(root) {
            TestRun::Failed(why) => Verdict::RunnerFailed(why),
            // Fewer binaries reported than at baseline: something did not run, and a
            // mutation the missing binary would have caught reads as survived.
            TestRun::Completed { result_lines, .. } if result_lines < baseline_results => {
                Verdict::RunnerFailed(format!(
                    "only {result_lines} of {baseline_results} test binaries reported"
                ))
            }
            TestRun::Completed { failing: 0, .. } => Verdict::Survived,
            TestRun::Completed { failing, .. } => Verdict::Caught { failing },
        }
    } else {
        Verdict::DidNotBuild
    };

    let _ = fs::write(&path, &original);
    let _ = fs::remove_file(&backup);
    verdict
}

/// Restore a mutation a killed run left applied.
///
/// Returns what it restored, so the caller can say so rather than silently repairing and
/// leaving the operator wondering why the previous run's results looked wrong.
fn restore_stranded(root: &Path) -> Result<Option<String>, String> {
    let backup = backup_path(root);
    let Ok(record) = fs::read_to_string(&backup) else {
        return Ok(None);
    };
    let Some((file, rest)) = record.split_once('\n') else {
        return Err(format!("the backup at {} is malformed", backup.display()));
    };
    let Some((expected_mutated, original)) = rest.split_once('\n') else {
        return Err(format!("the backup at {} is malformed", backup.display()));
    };

    let path = root.join(file);
    let current = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {file} to check it before restoring: {e}"))?;

    // Only restore over the exact mutated content the backup expects. Anything else means
    // the file changed after the crash — by an editor, a rebase, another tool — and
    // overwriting it would destroy work to repair a problem the operator may not know
    // exists. Stop, keep both, and let a human decide.
    if digest(&current) != expected_mutated {
        return Err(format!(
            "a previous mutation run left {file} mutated, but its contents have changed \
             since. Refusing to restore over them. The original is preserved at {}; \
             compare it with the working copy and resolve by hand.",
            backup.display()
        ));
    }

    fs::write(&path, original).map_err(|e| format!("could not restore {file}: {e}"))?;
    let _ = fs::remove_file(&backup);
    Ok(Some(file.to_owned()))
}

/// FNV-1a, hex. Detects accidental divergence between a backup and a working copy, which
/// is all it is for — not a defence against anyone deliberately forging a match.
fn digest(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn builds(root: &Path) -> bool {
    // `--tests` matters: without it a broken test file compiles nothing, `cargo test`
    // reports nothing, and every mutation looks like it survived.
    Command::new(cargo())
        .arg("build")
        .arg("--workspace")
        .arg("--tests")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}

/// What one run of the suite actually produced.
enum TestRun {
    /// Ran to completion. `failing` may be zero.
    Completed { failing: usize, result_lines: usize },
    /// Did not run to completion, for a reason that is about the runner, not the code.
    Failed(String),
}

fn run_tests(root: &Path) -> TestRun {
    let out = match Command::new(cargo())
        .arg("test")
        .arg("--workspace")
        // Without this, cargo stops after the first test binary that fails — so a
        // *caught* mutation reports fewer result lines than the baseline and the
        // completeness check below misclassifies it as a runner error. Found by the
        // negative control for that very check, which reported RUNNER ERROR for
        // mutations that were in fact being caught correctly.
        .arg("--no-fail-fast")
        .current_dir(root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return TestRun::Failed(format!("could not start cargo test: {e}")),
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let result_lines = text
        .lines()
        .filter(|l| l.starts_with("test result:"))
        .count();

    if result_lines == 0 {
        // Exit code alone cannot distinguish this: cargo exits non-zero both for "tests
        // failed" and "nothing ran". Only the absence of result lines separates them.
        let stderr = String::from_utf8_lossy(&out.stderr);
        return TestRun::Failed(format!(
            "the suite produced no results at all ({}). stderr: {}",
            out.status,
            stderr.lines().last().unwrap_or("").trim()
        ));
    }

    // Killed by a signal. A non-zero *code* is expected and normal here — a caught
    // mutation makes tests fail — but a signal means the runner itself died, and the
    // results it did print describe a partial run.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(sig) = out.status.signal() {
            return TestRun::Failed(format!("the test runner was killed by signal {sig}"));
        }
    }

    let failing = text
        .lines()
        .filter_map(|l| l.strip_prefix("test result: FAILED. "))
        .filter_map(|rest| rest.split(';').nth(1))
        .filter_map(|f| f.trim().strip_suffix(" failed"))
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();

    TestRun::Completed {
        failing,
        result_lines,
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}
