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

/// The advisory lock the runner holds for the length of a run.
///
/// Deliberately **not** the path the first protocol used. That protocol treated the
/// file's *existence* as the lock and unlinked it on release; this one keeps a file and
/// locks it. Sharing one path between the two is not a smaller change, it is a broken
/// one: the v2 file outlives its run, so an old runner's `create_new` would fail, it
/// would read the file, judge the holder dead or unattributable, and **delete and
/// recreate it** — at which point two v2 runners lock two different inodes and both
/// believe they are alone. A separate path makes that impossible.
fn lock_path(root: &Path) -> PathBuf {
    root.join("target").join(".nazm-mutation-lock.v2")
}

/// The first protocol's lock file. Read for interoperability, never locked.
fn legacy_lock_path(root: &Path) -> PathBuf {
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
/// # Ownership is the kernel's answer, not a pid's
///
/// The first protocol recorded a pid and reasoned about it: create the file exclusively,
/// and if that failed, decide from the pid inside whether the holder was gone. Two
/// defects followed from that shape and neither was the last one. `kill -0 0` succeeds,
/// so a malformed lock read as permanently held. Then reclaiming a dead holder's lock
/// meant `remove` followed by `create`, which is not atomic: two runners that both saw
/// the same dead holder could interleave — A removes, A creates, **B removes A's fresh
/// lock**, B creates — and both would run.
///
/// Every repair of that shape is a repair to a race the process cannot actually
/// arbitrate. So the decision is handed to the kernel: [`fs::File::try_lock`] on a file
/// that is opened, locked, and **kept open for the whole run**. There is exactly one
/// winner, the loser is told it lost, and a holder that dies has its lock released by the
/// kernel with no reclaim path to get wrong. The file itself is never unlinked, so it is
/// a stable object to lock rather than one whose identity changes under contention.
///
/// The pid written inside is **diagnostic only**. Nothing reads it to decide ownership.
///
/// # What the lock does not establish, and what nothing here establishes
///
/// Not quiescence. Rust opens files `O_CLOEXEC`, so the `cargo` processes this runner
/// spawns do *not* inherit the lock; a runner killed mid-build has its lock released
/// immediately while its `cargo` child is still running and still writing to the tree.
/// Measured on this platform, not assumed.
///
/// An earlier version of this comment went on to claim that the write-ahead record
/// covered the gap, because [`restore_stranded`] re-reads and re-digests the file before
/// writing. **That was wrong, and it is the same error this project already corrected
/// once for `(dev, ino)` in the harness snapshot code.** A digest establishes what a file
/// contained at the instant it was read. It cannot establish that nothing will write the
/// file afterwards, so it is a check against *prior* drift and not a quiescence proof.
///
/// There is no mechanism here that establishes quiescence, and the honest consequence is
/// that automatic recovery is disabled: a stranded record makes [`run_all`] refuse rather
/// than repair. See [`restore_stranded`] for the procedure that replaces it.
#[derive(Debug)]
pub(crate) struct RunLock {
    /// Held open for the lifetime of the run. Dropping it releases the kernel's lock;
    /// that is the whole release protocol, and the file stays on disk.
    file: fs::File,
    /// Removed on release so that a first-protocol runner stops seeing a held tree.
    legacy_marker: PathBuf,
}

/// Why the tree could not be taken. Every variant fails closed.
#[derive(Debug)]
enum AcquireError {
    /// Another v2 runner holds the lock. The ordinary case, and the only benign one.
    Contended,
    /// The filesystem does not implement advisory locking. Not contention, and not a
    /// licence to continue: without the lock there is nothing to stop a second runner.
    Unsupported(String),
    /// The lock file could not be opened or locked for a reason of access, not of
    /// contention.
    Permission(String),
    /// Anything else, reported as itself rather than folded into one of the above.
    Other(String),
    /// A first-protocol lock names a process that is alive. The protocols do not
    /// coordinate, so this refuses rather than guessing.
    LegacyRunner(u32),
    /// A first-protocol lock exists and cannot be attributed to any process.
    LegacyUnattributable(String),
}

impl AcquireError {
    fn describe(&self, root: &Path) -> String {
        match self {
            Self::Contended => format!(
                "another mutation run holds {}. Two runs corrupt each other's restores, \
                 and a concurrent `cargo test` would see a mutated tree and report \
                 failures that are not real.",
                lock_path(root).display()
            ),
            Self::Unsupported(why) => format!(
                "advisory locking is not available for {} ({why}). Refusing to run: \
                 without it nothing prevents a second runner from mutating the same tree. \
                 Run from a filesystem that supports file locking.",
                lock_path(root).display()
            ),
            Self::Permission(why) => format!(
                "cannot take the mutation lock at {} ({why}). Refusing rather than \
                 continuing unlocked.",
                lock_path(root).display()
            ),
            Self::Other(why) => format!(
                "cannot take the mutation lock at {} ({why}). Refusing rather than \
                 continuing unlocked.",
                lock_path(root).display()
            ),
            Self::LegacyRunner(pid) => format!(
                "a mutation run using the previous lock protocol appears to be active \
                 (pid {pid}, holding {}). The two protocols do not coordinate, so this \
                 run refuses rather than joining it.",
                legacy_lock_path(root).display()
            ),
            Self::LegacyUnattributable(why) => format!(
                "a lock file from the previous protocol is present at {} and cannot be \
                 attributed to a process ({why}). Refusing rather than reclaiming. If no \
                 mutation run is active, delete the file.",
                legacy_lock_path(root).display()
            ),
        }
    }
}

impl RunLock {
    fn acquire(root: &Path) -> Result<Self, AcquireError> {
        let path = lock_path(root);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Opened read+write and created if absent, never truncated on open: another
        // runner may be holding a lock on this very file, and truncating it before
        // asking for the lock would rewrite an object someone else owns.
        let file = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| classify_io(&e))?;

        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(AcquireError::Contended),
            Err(fs::TryLockError::Error(e)) => return Err(classify_io(&e)),
        }

        // Held from here. Everything below may fail, and dropping `file` on the way out
        // releases the kernel's lock, so no path leaves the tree marked as taken.
        let legacy = legacy_lock_path(root);
        claim_legacy_marker(&legacy)?;

        // Diagnostic only. Written after the lock is held, so its contents can never be
        // what another runner consults to decide whether it may proceed.
        let _ = write_pid(&file);

        Ok(Self {
            file,
            legacy_marker: legacy,
        })
    }
}

/// Separate "someone else has it" from "this cannot work here" from "I am not allowed".
///
/// `ENOTSUP`/`EOPNOTSUPP` from some network filesystems, and `EINVAL` from a few others,
/// mean the call will never succeed — silently proceeding there would leave the runner
/// with no mutual exclusion at all on exactly the filesystems where a shared checkout is
/// most likely.
fn classify_io(e: &std::io::Error) -> AcquireError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => AcquireError::Permission(e.to_string()),
        ErrorKind::Unsupported => AcquireError::Unsupported(e.to_string()),
        _ => match e.raw_os_error() {
            // ENOTSUP and EOPNOTSUPP share a value on macOS (45); on Linux EOPNOTSUPP is
            // 95. EINVAL (22) is what some filesystems return for an unlockable file.
            Some(45 | 95 | 22) => AcquireError::Unsupported(e.to_string()),
            _ => AcquireError::Other(e.to_string()),
        },
    }
}

fn write_pid(file: &fs::File) -> std::io::Result<()> {
    use std::io::{Seek, Write as _};
    let mut f = file;
    f.set_len(0)?;
    f.rewind()?;
    writeln!(f, "{}", std::process::id())?;
    f.flush()
}

/// Take the first protocol's marker so that an old runner is *likely* to refuse to start.
///
/// # This is mitigation, not compatibility
///
/// **The two protocols do not coordinate, and this does not make them.** One treats a
/// file's existence as the lock; the other locks a file. There is no sequence of
/// unprivileged filesystem operations that makes those two agree, because the v1 side's
/// logic is fixed and does not consult a lock.
///
/// The residual race is concrete and worth naming rather than implying away: a v2 runner
/// reads the marker and finds it absent, a v1 runner publishes its own marker, and the v2
/// runner then writes over it — both proceed. Creating the marker exclusively (below)
/// removes exactly that interleaving, but a v1 runner that has already passed its own
/// existence check and not yet published is still unaccounted for.
///
/// **The supported boundary is therefore: do not run a v1 and a v2 runner against the
/// same tree.** What this code buys is that the common cases — a v1 marker already
/// present, or a v1 runner starting after this one — are caught, and that a stale v1
/// marker does not need a human. It is consulted only *after* the real lock is held, so
/// no v2 runner's ownership decision depends on it.
fn claim_legacy_marker(path: &Path) -> Result<(), AcquireError> {
    match fs::read_to_string(path) {
        Err(_) => {}
        Ok(existing) => match holder_state(existing.trim()) {
            // Holding the v2 lock already proves no v2 runner is here, so a live pid is
            // either a v1 runner or an unrelated process that inherited the number.
            // Either way it is not this run's to take.
            Holder::Alive(pid) if pid != std::process::id() => {
                return Err(AcquireError::LegacyRunner(pid))
            }
            Holder::Alive(_) => {}
            Holder::Dead(pid) => {
                println!(
                    "  cleared      a first-protocol lock left by pid {pid}, which is no \
                     longer running"
                );
                let _ = fs::remove_file(path);
            }
            Holder::Unknown(why) => return Err(AcquireError::LegacyUnattributable(why)),
        },
    }
    // `create_new`, not `write`: overwriting would clobber a marker a v1 runner published
    // between the read above and this line, and tell it that it holds a tree we are also
    // about to mutate. If it lost the create, it is because someone else published first,
    // and that someone must be judged rather than overwritten.
    match fs::File::options().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            use std::io::Write as _;
            let _ = writeln!(f, "{}", std::process::id());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            match holder_state(fs::read_to_string(path).unwrap_or_default().trim()) {
                Holder::Alive(pid) if pid != std::process::id() => {
                    Err(AcquireError::LegacyRunner(pid))
                }
                Holder::Alive(_) | Holder::Dead(_) => Ok(()),
                Holder::Unknown(why) => Err(AcquireError::LegacyUnattributable(why)),
            }
        }
        // The marker is advisory. Failing to write it is not a reason to refuse a tree
        // whose real lock we hold.
        Err(_) => Ok(()),
    }
}

/// What can be established about whoever holds the *legacy* marker.
///
/// Retained from the first protocol, narrowed to the one job it is still fit for. It is
/// no longer an ownership decision for this runner — [`RunLock`] gets that from the
/// kernel — only a judgement about whether a v1 runner may be active.
#[derive(Debug)]
enum Holder {
    Alive(u32),
    Dead(u32),
    /// Neither could be established. Not the same as dead, and must not be treated as
    /// it: a probe that failed says nothing about the process.
    Unknown(String),
}

fn holder_state(pid: &str) -> Holder {
    // Validated before any probe. `kill -0 0` addresses the current process group and
    // succeeds; a negative number addresses another group. Neither answers the question
    // being asked, so neither reaches the probe.
    let Ok(n) = pid.parse::<u32>() else {
        return Holder::Unknown(if pid.is_empty() {
            "the lock is empty".to_owned()
        } else {
            format!("`{pid}` is not a process id")
        });
    };
    if n == 0 {
        return Holder::Unknown("0 is not a process id".to_owned());
    }
    process_exists(n)
}

/// Does this process exist?
///
/// `ps` rather than `kill -0`, because the answer has to distinguish "no such process"
/// from "exists but is not yours". `kill -0` fails for both, and through the `kill`
/// binary there is no errno to tell them apart — so a lock held by another user's live
/// run would look dead and be stolen.
fn process_exists(pid: u32) -> Holder {
    match Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Holder::Alive(pid),
        // `ps` ran and found nothing. That is an answer.
        Ok(_) => Holder::Dead(pid),
        // `ps` could not run at all. That is not.
        Err(e) => Holder::Unknown(format!("could not probe pid {pid}: {e}")),
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        // The legacy marker is existence-based, so releasing it means removing it. The
        // v2 lock file is *not* removed: it is the object the next runner locks, and
        // unlinking it under a waiting process is how the old protocol lost mutual
        // exclusion.
        let _ = fs::remove_file(&self.legacy_marker);
        // Closing the handle would release the lock anyway. Unlocking explicitly says so
        // at the point it happens, and keeps the release from depending on exactly when
        // the last clone of the descriptor goes away.
        let _ = self.file.unlock();
    }
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
    /// A test binary died — stack overflow, abort, segfault — while the mutation was
    /// injected.
    ///
    /// Distinct from both `Caught` and `RunnerFailed`, and the distinction is the point.
    /// The mutant did have an observable effect, so this is not `Survived`; but nothing
    /// *asserted* anything, so it is not `Caught` either, and the binary that died took
    /// its other results with it. It is also not a `RunnerFailed`: the orchestration
    /// worked perfectly and reported exactly what happened. The fix is always the same —
    /// move the case to a suite that runs the program as a child process, where a crash
    /// is an exit code the parent can assert on.
    MutantCrashed { signature: String, missing: usize },
    /// The mutation was injected and could **not** be taken back out.
    ///
    /// Fatal to the run, and the only verdict that is. Every other outcome leaves the
    /// tree as it was found; this one leaves it mutated, so continuing would inject the
    /// next defect on top of the last one and measure both at once. The record is kept,
    /// which is what makes the tree recoverable afterwards.
    NotRestored(String),
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

/// Take the tree's lock, for the hidden `mutate-hold` fixture.
///
/// The same acquisition a real run performs — the fixture exists so a *second* process
/// can be shown to lose, which cannot be demonstrated from inside the one that won.
pub(crate) fn acquire_lock(root: &Path) -> Result<RunLock, String> {
    RunLock::acquire(root).map_err(|e| e.describe(root))
}

/// Recover an interrupted run. The operator's action, not the runner's.
///
/// `confirmed` is the operator asserting that nothing is still writing to this tree —
/// the step [`run_all`] cannot take for itself and no longer pretends to. Without it,
/// this prints the procedure and changes nothing.
///
/// The lock is taken first and held across the restore, so a *new* run cannot start
/// underneath. That bounds v2 runners and nothing else; it says nothing about a dead
/// run's surviving children, which is exactly why the attestation is required.
pub(crate) fn recover(root: &Path, confirmed: bool) -> Result<String, String> {
    let _lock = acquire_lock(root)?;
    if !confirmed && backup_path(root).exists() {
        return Err(stranded_refusal(root));
    }
    Ok(match restore_stranded(root)? {
        Recovery::Repaired(f) => format!("repaired {f}"),
        Recovery::AlreadyRestored(f) => format!("already-restored {f}"),
        Recovery::Nothing => "nothing to recover".to_owned(),
    })
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

    // Bound to a name that lives to the end of this function on purpose. The kernel
    // lock is released when this handle drops, so it has to outlive recovery, every
    // injection, every test run, and every restoration — not just acquisition.
    let run_lock = match RunLock::acquire(root) {
        Ok(l) => l,
        Err(e) => return Err(vec![e.describe(root)]),
    };

    // A previous run may have been killed mid-mutation. This used to repair it here and
    // carry on. It no longer does, and the reason is the one thing this runner cannot
    // establish: **whether the dead run's descendants have stopped.**
    //
    // The lock says nothing about them — it is released by the kernel the instant the
    // coordinator dies, while its `cargo` and `rustc` children keep running and keep
    // writing. Neither does the record: a digest says what a file held when it was read,
    // not that nothing will write it next. Repairing under a live writer, and then
    // injecting the next mutation on top, is how a run produces verdicts about a file
    // two processes are editing.
    //
    // So recovery becomes an operator action, taken once quiescence has actually been
    // checked. `restore_stranded` is unchanged and still correct about contents; it is
    // simply no longer called by something that cannot know it is safe to call it.
    if backup_path(root).exists() {
        return Err(vec![stranded_refusal(root)]);
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
    let baseline_results = match run_tests(root, 0) {
        TestRun::Failed(why) => {
            return Err(vec![format!(
                "the baseline suite did not run to completion ({why}). Every verdict \
                 below would be meaningless."
            )])
        }
        // Nothing is injected yet, so this is the tree's own doing, not a mutant's.
        TestRun::Crashed { signature, .. } => {
            return Err(vec![format!(
                "the unmutated suite killed a test binary ({signature}). Fix that first: \
                 a baseline that cannot survive itself measures nothing."
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
            Verdict::MutantCrashed { signature, missing } => {
                println!("  MUTANT CRASH {}", m.name);
                problems.push(format!(
                    "{}: the mutant killed a test binary ({signature}), taking {missing} \
                     binary's results with it. The defect had an effect, but no test \
                     asserted it and the rest of that binary went unmeasured — move the \
                     case out of process, where a crash is an exit code to assert on.",
                    m.name
                ));
            }
            Verdict::NotRestored(why) => {
                println!("  NOT RESTORED {}", m.name);
                problems.push(format!(
                    "{}: {why} The run stopped here: every later verdict would have been \
                     measured against a file still holding this mutation.",
                    m.name
                ));
                // Deliberately not a `continue`. The tree is damaged.
                break;
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

    // Every mutation has been injected and restored by now. Released explicitly so the
    // extent of the critical section is visible here rather than inferred from scope.
    drop(run_lock);

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Why this run will not start, and exactly what to do about it.
///
/// Deliberately a procedure rather than a suggestion. "Resolve by hand" without saying
/// how is what makes an operator reach for `rm` on whichever file looks suspicious.
fn stranded_refusal(root: &Path) -> String {
    format!(
        "a previous mutation run was interrupted and left a recovery record at {}. \n\
         \n\
         This run will not start, and recovery is not automatic. Killing a run releases \n\
         its lock immediately, but its `cargo` and `rustc` descendants are not killed \n\
         with it and keep writing to this tree. Nothing here can establish that they \n\
         have stopped: a pid may have been reused, a matching digest only describes the \n\
         instant it was read, and a delay proves nothing at all. Restoring the source \n\
         while one of them is still running, and then injecting the next mutation on \n\
         top, would produce verdicts about a file two processes are editing.\n\
         \n\
         To recover:\n\
         \n\
         1. Establish that nothing is still writing to this tree. `pgrep -g <pgid>` for \n\
            the interrupted run's process group, or `pgrep -fl '{}'`, will *help you \n\
            look* — but neither is proof. A process may have changed its group, been \n\
            re-parented, hold a descriptor under a path that does not match, or be about \n\
            to start. Treat them as investigative aids.\n\
         2. Run `cargo xtask mutate-recover --confirm-no-writers`, which restores the \n\
            original bytes only if the file still matches what the record describes, and \n\
            refuses if it does not.\n\
         3. Re-run `cargo xtask mutate`.\n\
         \n\
         The flag in step 2 is **your attestation that the workspace is quiet**, not a \n\
         check this tool performs. It has no way to perform one.",
        backup_path(root).display(),
        root.display()
    )
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
    let record = format!(
        "{RECORD_VERSION}\n{}\n{}\n{}\n{original}",
        m.file,
        digest(&mutated),
        digest(&original)
    );
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
        match run_tests(root, baseline_results) {
            TestRun::Failed(why) => Verdict::RunnerFailed(why),
            TestRun::Crashed {
                signature,
                result_lines,
            } => Verdict::MutantCrashed {
                signature,
                missing: baseline_results.saturating_sub(result_lines),
            },
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

    finish(&path, &original, &backup, &m.file, verdict, |p, c| {
        fs::write(p, c)
    })
}

/// Put the original back, then clear the record — in that order, and only in that order.
///
/// The write is taken as a parameter so a test can fail it at the exact boundary that
/// matters. That is a smaller seam than a filesystem trait: there is one call, its
/// failure has one consequence, and the consequence is what needs a regression.
///
/// Clearing the record regardless of whether the restore succeeded — which is what this
/// did — converts the one recoverable failure into an unrecoverable one. The file stays
/// mutated and the write-ahead record that would have repaired it is gone, so the next
/// run measures a damaged tree with no way to know.
fn finish(
    path: &Path,
    original: &str,
    backup: &Path,
    file: &str,
    verdict: Verdict,
    write: impl FnOnce(&Path, &str) -> std::io::Result<()>,
) -> Verdict {
    match write(path, original) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            verdict
        }
        Err(e) => Verdict::NotRestored(format!(
            "{file} was mutated and could not be restored ({e}). Its original is \
             preserved at {} and has deliberately been kept. Recover with \
             `cargo xtask mutate-recover --confirm-no-writers` once nothing is still \
             writing to this tree.",
            backup.display()
        )),
    }
}

/// Marks the write-ahead record's layout, so a record this runner did not write is
/// reported rather than misread.
///
/// The first layout was `file\ndigest(mutated)\noriginal`, whose first line is a path —
/// distinguishable from this marker, which is the point. A record left by an older runner
/// is a state this one cannot safely interpret, and guessing at it would risk writing the
/// wrong bytes over a source file.
const RECORD_VERSION: &str = "nazm-mutation-backup 2";

/// What startup recovery found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recovery {
    /// A stranded mutation was found and the original restored.
    Repaired(String),
    /// The source was already the original; only the record was stranded. The run was
    /// killed in the window between restoring the file and clearing its record.
    AlreadyRestored(String),
    /// Nothing to do.
    Nothing,
}

/// Restore a mutation a killed run left applied.
///
/// Returns what it did, so the caller can say so rather than silently repairing and
/// leaving the operator wondering why the previous run's results looked wrong.
///
/// # Why the already-restored case is not drift
///
/// `apply_and_test` restores the file and *then* removes the record, and a process killed
/// between those two writes leaves an intact tree with a record still describing it. The
/// first version of this function compared the file against the mutated digest only, so
/// that state failed the comparison and was reported as "its contents have changed since
/// … resolve by hand" — a demand for manual intervention on a tree that was already
/// correct. Recording the original's digest as well is what separates "already restored"
/// from "somebody edited this".
fn restore_stranded(root: &Path) -> Result<Recovery, String> {
    let backup = backup_path(root);
    let Ok(record) = fs::read_to_string(&backup) else {
        return Ok(Recovery::Nothing);
    };

    let mut lines = record.splitn(5, '\n');
    let malformed = || {
        format!(
            "the backup at {} is malformed. It was not written by this runner, or it was \
             truncated. Nothing has been restored; inspect it and the working tree by hand.",
            backup.display()
        )
    };

    if record.trim().is_empty() {
        return Err(malformed());
    }

    match lines.next() {
        Some(RECORD_VERSION) => {}
        Some(_) => {
            return Err(format!(
                "the backup at {} was written by an earlier version of this runner, whose \
                 record layout this one cannot safely interpret. It names a file and holds \
                 its original contents after the second line. Restore it by hand, then \
                 delete it.",
                backup.display()
            ))
        }
        None => return Err(malformed()),
    }

    let (Some(file), Some(expected_mutated), Some(expected_original), Some(original)) =
        (lines.next(), lines.next(), lines.next(), lines.next())
    else {
        return Err(malformed());
    };

    let path = root.join(file);
    let current = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {file} to check it before restoring: {e}"))?;
    let current_digest = digest(&current);

    // Already correct: clear the record and say so. Writing the original again would be
    // harmless but would also report a repair that did not happen.
    if current_digest == expected_original {
        let _ = fs::remove_file(&backup);
        return Ok(Recovery::AlreadyRestored(file.to_owned()));
    }

    // Only restore over the exact mutated content the record expects. Anything else means
    // the file changed after the crash — by an editor, a rebase, another tool — and
    // overwriting it would destroy work to repair a problem the operator may not know
    // exists. Stop, keep both, and let a human decide.
    if current_digest != expected_mutated {
        return Err(format!(
            "a previous mutation run left {file} mutated, but its contents match neither \
             the mutation it applied nor the original it saved. Refusing to restore over \
             them. The original is preserved at {}; compare it with the working copy and \
             resolve by hand.",
            backup.display()
        ));
    }

    fs::write(&path, original).map_err(|e| format!("could not restore {file}: {e}"))?;
    let _ = fs::remove_file(&backup);
    Ok(Recovery::Repaired(file.to_owned()))
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
    /// A test binary died rather than reporting. Carries what it died of, and how many
    /// binaries did report, so the caller can say how much of the suite went unmeasured.
    Crashed {
        signature: String,
        result_lines: usize,
    },
    /// Did not run to completion, for a reason that is about the runner, not the code.
    Failed(String),
}

/// What a test binary died of, if one did.
///
/// Only meaningful once a binary is known to be missing: a crash and a broken
/// orchestration both show up as missing results, and the message is what separates them.
/// One is a fact about the code under test, the other about the runner, and reporting the
/// first as the second hides a real effect behind an apology.
///
/// The message alone proves nothing, which is the trap. The CLI suite runs `nazm` as a
/// child process *so that* a stack overflow is contained, and a test asserting on the
/// child's stderr prints that stderr — crash text included — while every binary reports
/// normally. Matching on the text before checking completeness turned three correct
/// catches into crashes.
fn crash_signature(stdout: &str, stderr: &str) -> Option<&'static str> {
    [
        "fatal runtime error: stack overflow",
        "has overflowed its stack",
        "memory allocation of",
        "SIGSEGV",
        "SIGABRT",
        "signal: 6",
        "signal: 11",
    ]
    .into_iter()
    .find(|sig| stderr.contains(sig) || stdout.contains(sig))
}

/// Run the suite. `baseline_results` is how many binaries reported when nothing was
/// injected; pass 0 to measure the baseline itself.
fn run_tests(root: &Path, baseline_results: usize) -> TestRun {
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

    let stderr_text = String::from_utf8_lossy(&out.stderr);
    let failing = text
        .lines()
        .filter_map(|l| l.strip_prefix("test result: FAILED. "))
        .filter_map(|rest| rest.split(';').nth(1))
        .filter_map(|f| f.trim().strip_suffix(" failed"))
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();

    // Only now, with a count of the binaries that reported, can a crash be attributed.
    // Crash text on its own means nothing: the CLI suite runs `nazm` as a child process
    // precisely so that a stack overflow is contained, and when a test there asserts on
    // the child's stderr it *prints that stderr*, crash message and all. Every binary
    // still reported, so nothing died here — a test caught the defect exactly as
    // intended. Reading the message alone turned three correct catches into crashes.
    if result_lines < baseline_results {
        if let Some(sig) = crash_signature(&text, &stderr_text) {
            return TestRun::Crashed {
                signature: sig.to_owned(),
                result_lines,
            };
        }
    }

    TestRun::Completed {
        failing,
        result_lines,
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::crash_signature;

    use super::{
        backup_path, digest, legacy_lock_path, lock_path, restore_stranded, AcquireError, Recovery,
        RunLock,
    };
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// A root with one source file, mutated, and the write-ahead record describing it —
    /// the state a run that was killed mid-mutation leaves behind.
    fn stranded(original: &str, mutated: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let src = root.path().join("src.rs");
        fs::write(&src, mutated).expect("write mutated");
        fs::create_dir_all(root.path().join("target")).expect("target");
        fs::write(backup_path(root.path()), record_for(original, mutated)).expect("record");
        (root, src)
    }

    fn record_for(original: &str, mutated: &str) -> String {
        format!(
            "{}\nsrc.rs\n{}\n{}\n{original}",
            super::RECORD_VERSION,
            digest(mutated),
            digest(original)
        )
    }

    #[test]
    fn an_interrupted_run_is_repaired_at_startup() {
        // The interruption case, which happened twice in one afternoon: a run killed
        // between injecting and restoring leaves the tree damaged, and the next thing to
        // read it — a test suite, a reviewer, an editor — sees a defect nobody wrote.
        let (root, src) = stranded("fn f() -> u8 { 1 }\n", "fn f() -> u8 { 2 }\n");

        assert_eq!(
            restore_stranded(root.path()).expect("must repair"),
            Recovery::Repaired("src.rs".to_owned()),
            "and must say what it repaired"
        );
        assert_eq!(fs::read_to_string(&src).unwrap(), "fn f() -> u8 { 1 }\n");
    }

    #[test]
    fn repairing_removes_the_record_it_created() {
        // "Including removal of files created by the run": a record left behind makes
        // the *next* run refuse to start, having already done its job.
        let (root, _src) = stranded("original\n", "mutated\n");
        assert!(backup_path(root.path()).exists(), "precondition");

        restore_stranded(root.path()).expect("must repair");
        assert!(
            !backup_path(root.path()).exists(),
            "the write-ahead record outlived the repair it described"
        );
    }

    #[test]
    fn an_already_restored_source_with_a_stranded_record_is_not_a_conflict() {
        // The crash window between `fs::write(original)` and `fs::remove_file(backup)`.
        // The tree is already correct and only the record is stale — but the first
        // version of this function compared against the *mutated* digest alone, so it
        // read the original as "changed since" and demanded a hand resolution on a tree
        // that needed none. Recording the original's digest too is what tells the two
        // apart.
        let (root, src) = stranded("original\n", "mutated\n");
        fs::write(&src, "original\n").expect("simulate the completed restore");

        assert_eq!(
            restore_stranded(root.path()).expect("must not be an error"),
            Recovery::AlreadyRestored("src.rs".to_owned()),
            "an already-repaired tree is not a conflict"
        );
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "original\n",
            "and nothing should have been rewritten"
        );
        assert!(
            !backup_path(root.path()).exists(),
            "the stale record must be cleared, or the next run trips on it again"
        );
    }

    #[test]
    fn a_file_edited_since_the_interruption_is_never_clobbered() {
        // The exact incident this guard was written for. After a killed run the file was
        // restored by hand; the record then no longer matched, and the runner refused
        // rather than overwriting the repair. Losing someone's work to fix a problem
        // they had already fixed is worse than stopping.
        //
        // "By hand" has to mean *differently* from the recorded original, or it is the
        // already-restored case above rather than drift.
        let (root, src) = stranded("original\n", "mutated\n");
        fs::write(&src, "repaired by hand\n").expect("hand repair");

        let err = restore_stranded(root.path()).expect_err("must refuse");
        assert!(err.contains("match neither"), "{err}");
        assert!(
            err.contains("resolve by hand"),
            "it must say what to do: {err}"
        );

        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "repaired by hand\n",
            "the refusal overwrote the very work it was protecting"
        );
        assert!(
            backup_path(root.path()).exists(),
            "and the original must be kept for the comparison it asks for"
        );
    }

    #[test]
    fn a_clean_tree_needs_no_repair_and_says_nothing() {
        // No record, nothing to do — and no noise implying something was wrong.
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            restore_stranded(root.path()).expect("clean"),
            Recovery::Nothing
        );
    }

    #[test]
    fn a_malformed_record_is_reported_not_ignored() {
        // A truncated record — killed mid-write — means a file may still be mutated with
        // no way to know which. Silence would let the next run measure a damaged tree.
        let v = super::RECORD_VERSION;
        for bad in [
            String::new(),
            v.to_owned(),
            format!("{v}\nsrc.rs"),
            format!("{v}\nsrc.rs\nonedigest"),
        ] {
            let root = tempfile::tempdir().expect("tempdir");
            fs::create_dir_all(root.path().join("target")).expect("target");
            fs::write(backup_path(root.path()), &bad).expect("write");
            match restore_stranded(root.path()) {
                Err(e) => assert!(e.contains("malformed"), "{bad:?}: {e}"),
                // An empty file is indistinguishable from none by `read_to_string`, so
                // `Nothing` is acceptable there and only there.
                Ok(Recovery::Nothing) => assert!(bad.is_empty(), "silently ignored {bad:?}"),
                Ok(other) => panic!("claimed {other:?} from a malformed record {bad:?}"),
            }
        }
    }

    #[test]
    fn a_record_from_the_previous_protocol_is_reported_not_guessed_at() {
        // The first layout was `file\ndigest(mutated)\noriginal`. Read as the current one
        // it would take the *original's first line* for a digest and the rest for the
        // file's contents — writing truncated bytes over a source file. Refusing is the
        // only safe reading of a record this version did not write.
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("target")).expect("target");
        fs::write(root.path().join("src.rs"), "mutated\n").expect("src");
        fs::write(
            backup_path(root.path()),
            format!("src.rs\n{}\noriginal\n", digest("mutated\n")),
        )
        .expect("write v1 record");

        let err = restore_stranded(root.path()).expect_err("must refuse");
        assert!(err.contains("earlier version"), "{err}");
        assert_eq!(
            fs::read_to_string(root.path().join("src.rs")).unwrap(),
            "mutated\n",
            "a record it cannot read must not cause a write"
        );
    }

    #[test]
    fn a_record_naming_a_missing_file_reports_rather_than_panicking() {
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("target")).expect("target");
        fs::write(
            backup_path(root.path()),
            format!(
                "{}\ngone.rs\n{}\n{}\noriginal\n",
                super::RECORD_VERSION,
                digest("mutated"),
                digest("original\n")
            ),
        )
        .expect("write");

        let err = restore_stranded(root.path()).expect_err("must report");
        assert!(err.contains("gone.rs"), "{err}");
    }

    #[test]
    fn a_restore_that_fails_keeps_the_record_and_stops_the_run() {
        // The one failure that turns a recoverable tree into an unrecoverable one. The
        // record must survive it — clearing the record after a failed restore leaves the
        // file mutated with nothing left to repair it from.
        use super::{finish, Verdict};

        let (root, src) = stranded("original\n", "mutated\n");
        let backup = backup_path(root.path());

        let verdict = finish(
            &src,
            "original\n",
            &backup,
            "src.rs",
            Verdict::Survived,
            |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected at the restoration boundary",
                ))
            },
        );

        // The failure propagates, and as the one verdict that is fatal to a run rather
        // than a result about a mutation.
        match &verdict {
            Verdict::NotRestored(why) => {
                assert!(why.contains("src.rs"), "{why}");
                assert!(why.contains("could not be restored"), "{why}");
                assert!(
                    why.contains("mutate-recover"),
                    "it must say how to fix it: {why}"
                );
            }
            _ => panic!("a failed restore must not be reported as a mutation result"),
        }

        assert!(
            backup.exists(),
            "the record was cleared after a failed restore"
        );
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "mutated\n",
            "precondition: the file is still mutated"
        );

        // And the tree is still recoverable, to the exact original bytes.
        assert_eq!(
            restore_stranded(root.path()).expect("must repair"),
            Recovery::Repaired("src.rs".to_owned())
        );
        assert_eq!(fs::read_to_string(&src).unwrap(), "original\n");
        assert!(!backup.exists());
    }

    #[test]
    fn a_successful_restore_clears_the_record() {
        // The other half, so the test above is not satisfied by a `finish` that never
        // clears anything.
        use super::{finish, Verdict};

        let (root, src) = stranded("original\n", "mutated\n");
        let backup = backup_path(root.path());

        let verdict = finish(
            &src,
            "original\n",
            &backup,
            "src.rs",
            Verdict::Caught { failing: 1 },
            |p, c| fs::write(p, c),
        );

        assert!(matches!(verdict, Verdict::Caught { failing: 1 }));
        assert_eq!(fs::read_to_string(&src).unwrap(), "original\n");
        assert!(
            !backup.exists(),
            "a successful restore must clear its record"
        );
    }

    #[test]
    fn a_second_acquisition_is_refused_while_one_is_held() {
        // Two runners corrupt each other's restores, and a concurrent `cargo test` sees
        // a mutated tree and reports failures that are not real — which is how the need
        // for this lock was found in the first place.
        //
        // An advisory lock belongs to the open file description, not to the process, so
        // a second `open` + `try_lock` here contends exactly as another process would.
        // The cross-*process* case is proved in `tests/lifecycle.rs`, because only a
        // separate process can show that a killed holder's lock is released.
        let root = tempfile::tempdir().expect("tempdir");
        let held = RunLock::acquire(root.path()).expect("first acquires");

        match RunLock::acquire(root.path()) {
            Err(AcquireError::Contended) => {}
            Err(other) => panic!("wrong reason: {other:?}"),
            Ok(_) => panic!("two runners acquired the same tree"),
        }

        drop(held);
        RunLock::acquire(root.path()).expect("released, so the next may start");
    }

    #[test]
    fn releasing_the_lock_keeps_its_file_and_lets_the_next_run_take_it() {
        // Deliberately the opposite of the assertion this replaces. The first protocol
        // unlinked its lock on release, and that was the defect: a file whose identity
        // changes under contention cannot be the thing two processes agree on. The v2
        // file is a stable object to lock, so what must be asserted is *release* — that
        // a later acquisition succeeds — not absence.
        let root = tempfile::tempdir().expect("tempdir");
        let lock = RunLock::acquire(root.path()).expect("acquire");
        assert!(lock_path(root.path()).exists(), "precondition");
        drop(lock);

        assert!(
            lock_path(root.path()).exists(),
            "the lock file is the coordination object and must survive release"
        );
        let again = RunLock::acquire(root.path()).expect("the lock must be free again");
        drop(again);
    }

    #[test]
    fn the_pid_inside_the_lock_is_diagnostic_only() {
        // It is written *after* the lock is held, and nothing consults it to decide
        // ownership. Corrupting it must therefore change nothing about who may run —
        // which is the property the first protocol did not have, where this same byte
        // string *was* the decision.
        let root = tempfile::tempdir().expect("tempdir");
        let lock = RunLock::acquire(root.path()).expect("acquire");
        let written = fs::read_to_string(lock_path(root.path())).expect("read");
        assert_eq!(written.trim(), std::process::id().to_string());
        drop(lock);

        for garbage in ["", "0", "-1", "not-a-pid", "99999999999999999999"] {
            fs::write(lock_path(root.path()), garbage).expect("corrupt");
            let taken = RunLock::acquire(root.path())
                .unwrap_or_else(|e| panic!("{garbage:?} changed the verdict: {e:?}"));
            drop(taken);
        }
    }

    #[test]
    fn a_first_protocol_lock_naming_a_live_process_is_refused() {
        // The migration case. The two protocols cannot coordinate — one locks a file,
        // the other treats existence as the lock — so the only safe reading of a v1
        // marker naming a live process is that a v1 runner may be in the tree.
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("target")).expect("target");
        // A process that is certainly alive and certainly not us, owned and reaped here
        // so the test leaves nothing running.
        let mut helper = Command::new("sleep").arg("30").spawn().expect("helper");
        fs::write(legacy_lock_path(root.path()), helper.id().to_string()).expect("v1 lock");

        let verdict = RunLock::acquire(root.path());

        let _ = helper.kill();
        let _ = helper.wait();

        match verdict {
            Err(AcquireError::LegacyRunner(_)) => {}
            Err(other) => panic!("wrong reason: {other:?}"),
            Ok(_) => panic!("joined a run using the previous protocol"),
        }
    }

    #[test]
    fn an_unattributable_first_protocol_lock_is_refused_not_stolen() {
        // Reclaiming an unattributable marker is fail-open: if its holder is in fact
        // alive, two runners end up on one tree. Refusing is fail-safe and costs an
        // operator one `rm`.
        for garbage in ["", "0", "-1", "not-a-pid", "99999999999999999999"] {
            let root = tempfile::tempdir().expect("tempdir");
            fs::create_dir_all(root.path().join("target")).expect("target");
            fs::write(legacy_lock_path(root.path()), garbage).expect("write");

            match RunLock::acquire(root.path()) {
                Err(AcquireError::LegacyUnattributable(_)) => {}
                Err(other) => panic!("{garbage:?}: wrong reason: {other:?}"),
                Ok(_) => panic!("a v1 marker holding {garbage:?} was stolen"),
            }
            assert!(
                legacy_lock_path(root.path()).exists(),
                "{garbage:?}: refusing must not remove the marker it refused to judge"
            );
        }
    }

    #[test]
    fn a_first_protocol_lock_from_a_dead_process_does_not_block() {
        // The other half: a marker left by a killed v1 run must not need a human before
        // anything can run again.
        let root = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(root.path().join("target")).expect("target");

        let mut child = Command::new("true").spawn().expect("spawn");
        let dead = child.id();
        child.wait().expect("reap");
        fs::write(legacy_lock_path(root.path()), dead.to_string()).expect("write");

        let lock = RunLock::acquire(root.path()).expect("a dead v1 marker must not block");
        assert_eq!(
            fs::read_to_string(legacy_lock_path(root.path()))
                .unwrap()
                .trim(),
            std::process::id().to_string(),
            "and the marker must now name us, so a v1 runner stands down"
        );
        drop(lock);
        assert!(
            !legacy_lock_path(root.path()).exists(),
            "the v1 marker is existence-based, so releasing it means removing it"
        );
    }

    #[test]
    fn a_dead_holder_is_distinguished_from_an_unprobeable_one() {
        use super::{holder_state, Holder};

        let mut child = Command::new("true").spawn().expect("spawn");
        let dead = child.id();
        child.wait().expect("reap");

        assert!(matches!(holder_state(&dead.to_string()), Holder::Dead(_)));
        assert!(matches!(
            holder_state(&std::process::id().to_string()),
            Holder::Alive(_)
        ));
        // Not dead. Nothing was established at all, and the two must not be conflated:
        // "the probe failed" is not evidence the process is gone.
        assert!(matches!(holder_state(""), Holder::Unknown(_)));
        assert!(matches!(holder_state("0"), Holder::Unknown(_)));
        assert!(matches!(holder_state("-1"), Holder::Unknown(_)));
    }

    #[test]
    fn a_live_holder_is_still_recognised() {
        // The other half: the fix must not make every marker reclaimable, which would
        // remove the protection entirely.
        use super::{holder_state, Holder};
        assert!(
            matches!(
                holder_state(&std::process::id().to_string()),
                Holder::Alive(_)
            ),
            "this very process should read as alive"
        );
    }

    #[test]
    fn the_digest_distinguishes_the_contents_it_is_asked_about() {
        // A regression test on these specific contents, and nothing more. It does not
        // establish collision resistance and is not meant to: FNV-1a has none, and the
        // function's own doc says so. What it pins is that the refusal above actually
        // discriminates for the inputs it will see in practice — a mutated file versus
        // a hand-repaired one.
        assert_eq!(digest("mutated\n"), digest("mutated\n"));
        assert_ne!(digest("mutated\n"), digest("repaired by hand\n"));
        assert_ne!(digest(""), digest("\n"));
        let _ = Path::new("");
    }

    #[test]
    fn a_stack_overflow_is_recognised_as_a_crash() {
        // Captured verbatim from a run where removing the parser's recursion bound
        // killed the nazm-syntax test binary.
        let stderr = "\nthread 'parser::nesting_tests::at_the_limit' (2568880) has \
                      overflowed its stack\nfatal runtime error: stack overflow, \
                      aborting\n";
        // Which of the two stack-overflow signatures matches first is list order, not
        // behaviour, and pinning it would make this a test of the list.
        let found = crash_signature("", stderr).expect("a stack overflow is a crash");
        assert!(found.contains("stack"), "unhelpful signature: {found}");
    }

    #[test]
    fn an_ordinary_test_failure_is_not_a_crash() {
        // The case that must not be swept into the crash bucket: a mutation being caught
        // exactly as intended. Reading this as a crash would turn coverage into an
        // apology about the runner.
        let stdout = "test result: FAILED. 21 passed; 1 failed; 0 ignored\n\
                      ---- parser::tests::x stdout ----\n\
                      assertion `left == right` failed\n";
        assert_eq!(crash_signature(stdout, ""), None);
    }

    #[test]
    fn a_clean_run_is_not_a_crash() {
        assert_eq!(
            crash_signature("test result: ok. 265 passed; 0 failed\n", ""),
            None
        );
    }

    #[test]
    fn a_crash_reported_on_stdout_is_still_a_crash() {
        // libtest writes some of this to stdout depending on capture settings, so
        // looking only at stderr would miss it.
        assert_eq!(
            crash_signature("fatal runtime error: stack overflow\n", ""),
            Some("fatal runtime error: stack overflow")
        );
    }

    #[test]
    fn a_contained_child_crash_is_not_a_dead_binary() {
        // The trap, as the CLI suite actually produces it: a test runs `nazm` on a
        // program that overflows, asserts on the exit status, fails its assertion, and
        // prints the child's stderr. `crash_signature` matches — which is why the caller
        // must not consult it unless a binary is genuinely missing.
        let stdout = "test recursion_runs ... FAILED\n\
                      ---- recursion_runs stdout ----\n\
                      got:\nthread 'nazm-eval' (3492985) has overflowed its stack\n\
                      fatal runtime error: stack overflow, aborting\n\
                      test result: FAILED. 11 passed; 1 failed; 0 ignored\n";
        assert!(
            crash_signature(stdout, "").is_some(),
            "the text is present, so the guard cannot be the text"
        );
        // The count is what decides, and here nothing is missing.
        assert_eq!(
            stdout
                .lines()
                .filter(|l| l.starts_with("test result:"))
                .count(),
            1
        );
    }

    #[test]
    fn a_signal_death_is_a_crash() {
        assert_eq!(
            crash_signature("", "error: test failed, signal: 6 (SIGABRT)"),
            Some("SIGABRT")
        );
    }
}
