//! Automated mutation verification: the lock, the write-ahead record, recovery, and the
//! campaign driver.
//!
//! Injects each catalogued defect, rebuilds, and requires the tests to fail, then restores
//! the file byte for byte and verifies that it did before moving on. What runs between
//! injection and restoration is decided by `campaign.rs` (tiers) and `plan.rs` (what each
//! mutant's evidence is); how it runs is `procs.rs`.
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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::campaign::{
    self, Deadlines, Executor as _, Identity, Journal, Observation, Record, Strategy, Verdict,
};
use crate::plan::{self, KillerSpec, Mutation, MutationPlan};
use crate::procs::{self, Ended, Invocation};

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

/// How a campaign is to be run. Built by the CLI; every field has a meaning in the
/// campaign's identity or its output location, and none is a shell fragment.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) root: PathBuf,
    pub(crate) catalogue: PathBuf,
    pub(crate) only: Vec<String>,
    pub(crate) strategy: Strategy,
    /// Where journals and logs go. Default `<root>/target/mutation-campaigns`.
    pub(crate) campaign_dir: Option<PathBuf>,
    /// Where to look for a journal to resume. Default: `campaign_dir`.
    pub(crate) resume_from: Option<PathBuf>,
    pub(crate) resume: bool,
    /// Stop starting new mutants after this long. The session ends *incomplete*, never
    /// clean, and the rest is resumable.
    pub(crate) budget: Option<Duration>,
    /// The root is a disposable copy: derived `.nazm/` caches may be removed from it.
    pub(crate) reset_derived: bool,
    pub(crate) deadlines: Deadlines,
    pub(crate) source_commit: Option<String>,
    /// Print journal lines on stdout for a supervisor to collect.
    pub(crate) export_journal: bool,
    /// Each step's internal parallelism. N14.1; serial unless a profile says otherwise,
    /// and part of the campaign's identity either way.
    pub(crate) parallelism: plan::Parallelism,
    /// Verify every decided mutant's declared killers in the same session, around the one
    /// injection the verdict is taken from: each passes pristine, fails on the mutant and
    /// passes restored. Not part of the campaign's identity: it changes no verdict and is
    /// not journaled. N32-H.
    pub(crate) verify_killers: bool,
    /// The Cargo target is the harness's own cache, which it may clean. The contained
    /// launcher says so; a checkout's target is never assumed to be one. N32-H2.
    pub(crate) disposable_target: bool,
}

impl Options {
    pub(crate) fn new(root: &Path, catalogue: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            catalogue: catalogue.to_path_buf(),
            only: Vec::new(),
            strategy: DEFAULT_STRATEGY,
            campaign_dir: None,
            resume_from: None,
            resume: false,
            budget: None,
            reset_derived: false,
            deadlines: Deadlines::default(),
            source_commit: None,
            export_journal: false,
            parallelism: plan::Parallelism::SERIAL,
            verify_killers: false,
            disposable_target: false,
        }
    }

    fn campaigns(&self) -> PathBuf {
        self.campaign_dir
            .clone()
            .unwrap_or_else(|| self.root.join("target").join("mutation-campaigns"))
    }
}

/// The strategy a campaign uses unless told otherwise.
pub(crate) const DEFAULT_STRATEGY: Strategy = Strategy::Targeted;

/// How a campaign ended.
#[derive(Debug)]
pub(crate) enum End {
    /// Every selected mutant was finalized as caught.
    Clean,
    /// Every selected mutant was finalized, and some verdicts are problems.
    Problems(Vec<String>),
    /// The session stopped before every mutant was finalized. Never reported as clean.
    Incomplete(Vec<String>),
    /// The campaign did not start, or stopped because the tree could not be trusted.
    Refused(Vec<String>),
}

/// Plan only: validate the catalogue statically and print what each mutant would run.
pub(crate) fn plan_only(
    root: &Path,
    catalogue: &Path,
    only: &[String],
) -> Result<String, Vec<String>> {
    let cat = plan::load(catalogue).map_err(|e| vec![e])?;
    let problems = plan::validate(root, &cat);
    if !problems.is_empty() {
        return Err(problems);
    }
    let selected = plan::select(&cat, only).map_err(|e| vec![e])?;
    let plans = plan::plan(&cat, &selected)?;
    Ok(plan::render(&plans))
}

fn toolchain(root: &Path) -> String {
    let f = procs::run(&Invocation {
        program: "rustc".to_owned(),
        args: vec!["-vV".to_owned()],
        cwd: root.to_path_buf(),
        env: Vec::new(),
        deadline: Duration::from_secs(60),
    });
    match f.ended {
        Ended::Exited(s) if s.success() => f.stdout.trim().to_owned(),
        _ => "unknown".to_owned(),
    }
}

/// The harness's own identity: its source when the root carries it, else the binary.
fn harness(root: &Path) -> String {
    let src = root.join("xtask");
    if src.is_dir() {
        if let Ok(d) = campaign::tree_digest(&src) {
            return d;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| fs::read(p).ok())
        .map_or_else(|| "unknown".to_owned(), |b| campaign::blake(&b))
}

/// The CPU quota this process runs under, as its cgroup states it — `cpu.max`, such as
/// `400000 100000` for four CPUs — or `unbounded` where there is no cgroup file to read.
/// Part of a campaign's identity since N14.1.
fn cpu_quota() -> String {
    fs::read_to_string("/sys/fs/cgroup/cpu.max")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "unbounded".to_owned())
}

/// Build and run the workspace suite on the pristine tree. Returns how many binaries
/// reported, which every later workspace-suite run must match.
/// The pristine tree builds, tests included, and building it leaves it as it was.
///
/// With a disposable target (N32-H2), a pristine build that fails is tried once more after
/// `cargo clean`: the persistent verification target is a cache, and a damaged one — an
/// object file rustc's incremental cache would reuse — must cost a rebuild, never a
/// session. A tree that still does not build is refused, as before.
fn pristine_build(o: &Options, identity_tree: &str) -> Result<(), String> {
    let run = |args: Vec<String>| {
        procs::run(&Invocation {
            program: campaign::cargo(),
            args,
            cwd: o.root.clone(),
            env: Vec::new(),
            deadline: Duration::from_secs(o.deadlines.build_secs),
        })
    };
    let mut build = run(plan::workspace_build(o.parallelism));
    let built = |b: &procs::Finished| matches!(&b.ended, Ended::Exited(s) if s.success());
    if !built(&build) && o.disposable_target {
        println!(
            "  pristine build       failed against the verification target; cleaning it and \
             building once more"
        );
        let _ = run(vec!["clean".to_owned()]);
        build = run(plan::workspace_build(o.parallelism));
    }
    match &build.ended {
        Ended::Exited(s) if s.success() => {}
        other => {
            return Err(format!(
                "the unmutated workspace does not build (including tests): {other:?}. Fix \
                 that first: every mutation would otherwise be reported as SURVIVED."
            ))
        }
    }
    // The identity was taken before building. If building changed the tree (a lockfile
    // written, a generated file), the identity no longer names what is being tested, and
    // nothing journaled under it could be trusted on resume.
    match campaign::tree_digest(&o.root) {
        Ok(after) if after == identity_tree => {}
        Ok(_) => {
            return Err(
                "building the pristine tree changed it, so the campaign identity taken \
                        before the build does not describe what was tested. Commit what the \
                        build writes (a `Cargo.lock`, a generated file) and run again."
                    .to_owned(),
            )
        }
        Err(e) => return Err(format!("cannot digest the tree: {e}")),
    }
    Ok(())
}

/// The workspace suite on the pristine tree: green, and how many binaries report. Only a
/// verdict that reaches the workspace suite, or a profile target, rests on it (N32-H2):
/// Tier 3 compares a mutant's run against this count, and a survivor is only a survivor of
/// a suite that was green without it. A session runs it once, and only when some mutant
/// needs it.
fn baseline_suite(o: &Options) -> Result<usize, String> {
    if o.reset_derived {
        campaign::reset_derived(&o.root);
    }
    let suite = procs::run(&Invocation {
        program: campaign::cargo(),
        args: plan::broad_suite(o.parallelism),
        cwd: o.root.clone(),
        env: Vec::new(),
        deadline: Duration::from_secs(o.deadlines.broad_secs),
    });
    match campaign::observe(&suite, 0, true) {
        Observation::TimedOut(why) => Err(format!(
            "the baseline suite timed out ({why}). Every verdict below would be meaningless."
        )),
        Observation::Failed(why) => Err(format!(
            "the baseline suite did not run to completion ({why}). Every verdict below \
             would be meaningless."
        )),
        Observation::Completed { failing, .. } if failing > 0 => Err(format!(
            "{failing} test(s) already fail before any mutation. A mutation cannot be shown \
             to be caught by a suite that is not green."
        )),
        Observation::Completed { result_lines, .. } => Ok(result_lines),
    }
}

/// Every killer the campaign may run exists in its built target, exactly once, and is
/// not ignored. Checked on the pristine build, before any mutant is injected.
///
/// The baseline suite being green then says each of them passes. That is the first half
/// of what a Tier-1 catch rests on: this test passed without the mutant.
fn verify_listed(o: &Options, plans: &[MutationPlan]) -> Result<(), Vec<String>> {
    let mut by_target: BTreeMap<plan::Target, Vec<(String, String)>> = BTreeMap::new();
    for p in plans {
        for k in &p.killers {
            if let Some(name) = &k.exact {
                by_target
                    .entry(k.target.clone())
                    .or_default()
                    .push((p.name.clone(), name.clone()));
            }
        }
    }
    let mut problems = Vec::new();
    for (target, wanted) in &by_target {
        let list = |ignored: bool| -> Result<BTreeSet<String>, String> {
            let f = procs::run(&Invocation {
                program: campaign::cargo(),
                args: target.list_args(ignored, o.parallelism),
                cwd: o.root.clone(),
                env: Vec::new(),
                deadline: Duration::from_secs(o.deadlines.list_secs),
            });
            match &f.ended {
                Ended::Exited(s) if s.success() => Ok(f
                    .stdout
                    .lines()
                    .filter_map(|l| l.strip_suffix(": test"))
                    .map(str::to_owned)
                    .collect()),
                other => Err(format!("listing {} failed: {other:?}", target.label())),
            }
        };
        let (all, ignored) = match (list(false), list(true)) {
            (Ok(a), Ok(i)) => (a, i),
            (Err(e), _) | (_, Err(e)) => {
                problems.push(e);
                continue;
            }
        };
        for (mutation, name) in wanted {
            if !all.contains(name) {
                problems.push(format!(
                    "{mutation}: killer `{name}` is not a test in {}. Stale killer metadata is \
                     a catalogue defect, and this campaign will not start on it.",
                    target.label()
                ));
            } else if ignored.contains(name) {
                problems.push(format!(
                    "{mutation}: killer `{name}` in {} is ignored, so the workspace suite \
                     does not run it",
                    target.label()
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

/// Run a campaign.
pub(crate) fn run_all(o: &Options) -> End {
    let started = Instant::now();
    let cat = match plan::load(&o.catalogue) {
        Ok(c) => c,
        Err(e) => return End::Refused(vec![e]),
    };
    let selected = match plan::select(&cat, &o.only) {
        Ok(s) => s,
        Err(e) => return End::Refused(vec![e]),
    };
    if !o.only.is_empty() {
        println!(
            "running {} of the catalogue, selected by {:?}",
            selected.len(),
            o.only
        );
    }
    let plans = match plan::plan(&cat, &selected) {
        Ok(p) => p,
        Err(e) => return End::Refused(e),
    };

    // Bound to a name that lives to the end of this function on purpose. The kernel
    // lock is released when this handle drops, so it has to outlive recovery, every
    // injection, every test run, and every restoration — not just acquisition.
    let run_lock = match RunLock::acquire(&o.root) {
        Ok(l) => l,
        Err(e) => return End::Refused(vec![e.describe(&o.root)]),
    };

    // A previous run may have been killed mid-mutation. Recovery is an operator's
    // action, taken once quiescence has actually been checked; see `stranded_refusal`.
    // This comes before anything reads the tree, so a journal can never be consulted
    // over a file that still holds a mutation.
    if backup_path(&o.root).exists() {
        return End::Refused(vec![stranded_refusal(&o.root)]);
    }

    // Identity, from the tree as it is now: pristine, because nothing is stranded.
    let tree = match campaign::tree_digest(&o.root) {
        Ok(t) => t,
        Err(e) => return End::Refused(vec![format!("cannot digest the tree: {e}")]),
    };
    let identity = Identity {
        schema: campaign::JOURNAL_SCHEMA.to_owned(),
        tree,
        catalogue: fs::read(&o.catalogue)
            .map_or_else(|_| "unreadable".to_owned(), |b| campaign::blake(&b)),
        harness: harness(&o.root),
        toolchain: toolchain(&o.root),
        strategy: o.strategy,
        selected: plans.iter().map(|p| p.name.clone()).collect(),
        deadlines: o.deadlines,
        reset_derived: o.reset_derived,
        parallelism: o.parallelism,
        cpu_quota: cpu_quota(),
    };
    let key = identity.key();
    let header = campaign::Header {
        kind: "campaign".to_owned(),
        key: key.clone(),
        identity: identity.clone(),
        source_commit: o.source_commit.clone(),
    };

    let mut prior = BTreeMap::new();
    if o.resume {
        let from = o.resume_from.clone().unwrap_or_else(|| o.campaigns());
        let path = from.join(&key).join("journal.jsonl");
        if path.exists() {
            match campaign::read_journal(&path, &identity) {
                Ok(r) => prior = r,
                Err(e) => return End::Refused(vec![e]),
            }
            println!(
                "resuming campaign {key}: {} finalized entries in {}",
                prior.len(),
                path.display()
            );
        } else {
            println!(
                "resume: no journal for campaign {key} under {}. Nothing is reused; a journal \
                 recorded against any other tree, catalogue, harness, toolchain, strategy, \
                 selection, deadline set or execution profile has a different key and is \
                 never read.",
                from.display()
            );
        }
    }
    let reuse = |name: &str| prior.get(name).filter(|r| Verdict::is_semantic(&r.verdict));

    let mut journal = match Journal::open(Some(&o.campaigns()), &header, o.export_journal) {
        Ok(j) => j,
        Err(e) => return End::Refused(vec![format!("cannot open the campaign journal: {e}")]),
    };
    println!(
        "campaign {key}: strategy {}, {} selected, root {}",
        o.strategy.word(),
        plans.len(),
        o.root.display()
    );

    let pending = plans.iter().filter(|p| reuse(&p.name).is_none()).count();
    let mut phase = Instant::now();
    let mut setup: Vec<(&'static str, f64)> = Vec::new();
    let lap = |setup: &mut Vec<(&'static str, f64)>, what: &'static str, phase: &mut Instant| {
        setup.push((what, phase.elapsed().as_secs_f64()));
        *phase = Instant::now();
    };
    let mut baseline_results: Option<usize> = None;
    let mut pristine = Pristine::new(&identity.tree, o.parallelism);
    if pending > 0 {
        if let Err(e) = pristine_build(o, &identity.tree) {
            return End::Refused(vec![e]);
        }
        lap(&mut setup, "pristine build", &mut phase);
        if o.strategy == Strategy::Targeted {
            if let Err(e) = verify_listed(o, &plans) {
                return End::Refused(e);
            }
            lap(&mut setup, "killer listing", &mut phase);
            // The mutation baseline: every declared killer a pending mutant may be decided
            // by, run once on the pristine tree. That it passes is the first half of a
            // Tier-1 catch.
            let unique: BTreeSet<plan::TestSelector> = plans
                .iter()
                .filter(|p| reuse(&p.name).is_none())
                .flat_map(|p| p.killers.iter().cloned())
                .collect();
            let failing = pristine.establish(o, &unique);
            lap(&mut setup, "pristine killers", &mut phase);
            println!(
                "  mutation baseline    {} unique killer(s) run once on the pristine tree",
                unique.len()
            );
            if !failing.is_empty() {
                return End::Refused(vec![format!(
                    "{} declared killer(s) do not pass on the pristine tree, so no mutant can be \
                     shown to be caught by them: {}",
                    failing.len(),
                    failing.join(", ")
                )]);
            }
        } else {
            match baseline_suite(o) {
                Ok(n) => baseline_results = Some(n),
                Err(e) => return End::Refused(vec![e]),
            }
            lap(&mut setup, "baseline suite", &mut phase);
        }
    }
    let mut summary = campaign::Summary {
        selected: plans.len(),
        setup_secs: started.elapsed().as_secs_f64(),
        ..campaign::Summary::default()
    };

    let mut problems = Vec::new();
    let mut finalized = BTreeSet::new();
    let mut not_reached = Vec::new();
    let mut stopped = false;
    // Killer verification in this session (verified, checked), the mutants whose verdict
    // was reused and so were not verified here, and each decided mutant's seconds.
    let mut killer_checks = (0usize, 0usize);
    let mut unverified: Vec<String> = Vec::new();
    let mut mutant_secs: Vec<f64> = Vec::new();
    // Mutants Tier 1 did not decide, with the verification their one Tier-1 injection gave.
    let mut deferred: Vec<(&MutationPlan, Vec<KillerCheck>)> = Vec::new();

    // Execution order (N32-H3): mutants of one package, then of one killer target, run
    // consecutively, so Cargo rebuilds one slice at a time; the key is structured and the
    // catalogue index breaks ties, so the order is deterministic. Only the order changes:
    // each mutant starts from the digest-checked pristine tree whatever ran before it, and
    // the selection, identity, journal and verdicts are the catalogue's.
    let execution = bucket_order(&plans, &o.root);
    println!(
        "  execution order      {} bucket(s): {}",
        execution.buckets.len(),
        execution.buckets.join(", ")
    );

    // Pass 1: Tier 1 for every targeted mutant with declared killers; the rest wait.
    for p in execution.order.iter().filter_map(|&i| plans.get(i)) {
        if let Some(r) = reuse(&p.name) {
            println!(
                "  {} {}  (reused from journal {key}: {})",
                word(&r.verdict),
                p.name,
                r.detail
            );
            if o.verify_killers && !p.killers.is_empty() {
                unverified.push(p.name.clone());
            }
            summary.add(r, true);
            finalized.insert(p.name.clone());
            if let Some(m) = cat.mutation.get(p.index) {
                if let Some(problem) = problem(&r.verdict, &r.detail, m) {
                    problems.push(problem);
                }
            }
            continue;
        }
        if o.strategy != Strategy::Targeted || p.killers.is_empty() {
            deferred.push((p, Vec::new()));
            continue;
        }
        if stopped || o.budget.is_some_and(|b| started.elapsed() >= b) {
            not_reached.push(p.name.clone());
            continue;
        }
        let Some(m) = cat.mutation.get(p.index) else {
            not_reached.push(p.name.clone());
            continue;
        };
        let logs =
            o.campaigns()
                .join(&key)
                .join("logs")
                .join(format!("{:03}-{}", p.index + 1, p.name));
        let ask = Ask {
            baseline_results: 0,
            mode: Mode::Direct,
            verify: o.verify_killers,
        };
        match run_one(o, p, m, &ask, Some(logs), &pristine) {
            Ok(One::Final(r, checks)) => {
                let mut sink = Sink {
                    journal: &mut journal,
                    problems: &mut problems,
                    summary: &mut summary,
                    killer_checks: &mut killer_checks,
                    mutant_secs: &mut mutant_secs,
                };
                sink.finish(&r, &checks, p, m);
                finalized.insert(p.name.clone());
            }
            Ok(One::Deferred(checks, secs)) => {
                println!(
                    "  deferred     {}  (every declared killer passed on the mutant)",
                    p.name
                );
                mutant_secs.push(secs);
                deferred.push((p, checks));
            }
            Err(why) => {
                println!("  NOT RESTORED {}", p.name);
                problems.push(format!(
                    "{}: {why} The run stopped here: every later verdict would have been \
                     measured against a file still holding this mutation.",
                    p.name
                ));
                not_reached.push(p.name.clone());
                // Deliberately not a `continue`. The tree is damaged.
                stopped = true;
            }
        }
    }

    // Pass 2: the full law — baseline suite, then every tier — for what Tier 1 left.
    let waiting: Vec<(&MutationPlan, Vec<KillerCheck>)> = deferred
        .into_iter()
        .filter(|(p, _)| !not_reached.contains(&p.name))
        .collect();
    if !waiting.is_empty() && !stopped && baseline_results.is_none() {
        println!(
            "  baseline suite       {} mutant(s) need a tier beyond the killers",
            waiting.len()
        );
        match baseline_suite(o) {
            Ok(n) => baseline_results = Some(n),
            Err(e) => return End::Refused(vec![e]),
        }
        lap(&mut setup, "baseline suite", &mut phase);
    }
    for (p, first) in waiting {
        if stopped || o.budget.is_some_and(|b| started.elapsed() >= b) {
            not_reached.push(p.name.clone());
            continue;
        }
        let Some(m) = cat.mutation.get(p.index) else {
            not_reached.push(p.name.clone());
            continue;
        };
        let logs =
            o.campaigns()
                .join(&key)
                .join("logs")
                .join(format!("{:03}-{}", p.index + 1, p.name));
        // Verified at its Tier-1 injection if it had one; otherwise here.
        let ask = Ask {
            baseline_results: baseline_results.unwrap_or(0),
            mode: Mode::Full,
            verify: o.verify_killers && first.is_empty(),
        };
        match run_one(o, p, m, &ask, Some(logs), &pristine) {
            Ok(One::Final(r, checks)) => {
                let checks = if first.is_empty() { checks } else { first };
                let mut sink = Sink {
                    journal: &mut journal,
                    problems: &mut problems,
                    summary: &mut summary,
                    killer_checks: &mut killer_checks,
                    mutant_secs: &mut mutant_secs,
                };
                sink.finish(&r, &checks, p, m);
                finalized.insert(p.name.clone());
            }
            Ok(One::Deferred(..)) => {
                problems.push(format!("internal: {} was deferred by the full law", p.name));
                not_reached.push(p.name.clone());
            }
            Err(why) => {
                println!("  NOT RESTORED {}", p.name);
                problems.push(format!(
                    "{}: {why} The run stopped here: every later verdict would have been \
                     measured against a file still holding this mutation.",
                    p.name
                ));
                not_reached.push(p.name.clone());
                stopped = true;
            }
        }
    }

    // The tree the session ends on is the tree it started from, byte for byte, or nothing
    // it decided can be trusted and the session says so.
    if !stopped && pending > 0 {
        match campaign::tree_digest(&o.root) {
            Ok(t) if t == identity.tree => {}
            Ok(_) => {
                problems.push(
                    "the tree does not digest to the one the session began with: a restoration \
                     left something behind"
                        .to_owned(),
                );
                stopped = true;
            }
            Err(e) => {
                problems.push(format!("cannot digest the tree: {e}"));
                stopped = true;
            }
        }
    }
    pristine.forget();
    let setup_line: Vec<String> = setup.iter().map(|(w, t)| format!("{w} {t:.1}s")).collect();

    // Every mutation has been injected and restored by now. Released explicitly so the
    // extent of the critical section is visible here rather than inferred from scope.
    drop(run_lock);

    summary.not_reached = not_reached.clone();
    summary.wall = started.elapsed().as_secs_f64();
    print!("{}", summary.render(&key, o.strategy));
    println!("  setup phases         {}", setup_line.join(", "));
    println!(
        "{}",
        session_timing(summary.setup_secs, &mutant_secs, summary.wall)
    );
    if o.verify_killers {
        println!(
            "  killer verification  {} of {} verified in this session{}",
            killer_checks.0,
            killer_checks.1,
            if unverified.is_empty() {
                String::new()
            } else {
                format!(
                    "; {} reused, not verified here: {}",
                    unverified.len(),
                    unverified.join(", ")
                )
            }
        );
        if !unverified.is_empty() {
            problems.push(format!(
                "{} mutation(s) had their verdict reused from the journal, so their killers were \
                 not verified in this session: {}",
                unverified.len(),
                unverified.join(", ")
            ));
        }
    }
    let names: Vec<String> = plans.iter().map(|p| p.name.clone()).collect();
    if !campaign::accounted(&names, &finalized, &not_reached) {
        problems.push(
            "internal: the campaign's accounting does not cover every selected mutation".to_owned(),
        );
    }

    if stopped {
        End::Refused(problems)
    } else if !not_reached.is_empty() || !unverified.is_empty() {
        problems.push(format!(
            "{} of {} selected mutations were not reached this session: {}. Resume with \
             `--resume`; nothing about them has been established.",
            not_reached.len(),
            plans.len(),
            not_reached.join(", ")
        ));
        End::Incomplete(problems)
    } else if problems.is_empty() {
        End::Clean
    } else {
        End::Problems(problems)
    }
}

/// The word a verdict prints as, padded like the legacy runner's.
fn word(v: &str) -> &'static str {
    match v {
        "caught" => "caught      ",
        "survived" => "SURVIVED    ",
        "not-injected" => "NOT INJECTED",
        "unusable" => "UNUSABLE    ",
        "mutant-crash" => "MUTANT CRASH",
        "timeout" => "TIMEOUT     ",
        _ => "RUNNER ERROR",
    }
}

/// The problem a finalized verdict represents, if any. The legacy wording, kept.
fn problem(verdict: &str, detail: &str, m: &Mutation) -> Option<String> {
    let name = &m.name;
    match verdict {
        "caught" => None,
        "survived" => Some(format!(
            "{name}: injected and compiled, but the suite still passed. {}",
            m.why
        )),
        "not-injected" => Some(format!(
            "{name}: {detail}. A mutation that does not apply proves nothing — fix the pattern \
             rather than reading the pass as coverage."
        )),
        "unusable" => Some(format!(
            "{name}: the mutated tree does not compile, so a failing suite would not mean the \
             defect was detected."
        )),
        "mutant-crash" => Some(format!(
            "{name}: the mutant killed a test binary ({detail}). The defect had an effect, but \
             no test asserted it and the rest of that binary went unmeasured — move the case \
             out of process, where a crash is an exit code to assert on."
        )),
        "timeout" => Some(format!(
            "{name}: {detail}. A timeout is neither caught nor survived; it is no verdict."
        )),
        _ => Some(format!(
            "{name}: the suite did not run to completion ({detail}). This is neither caught nor \
             survived — a run that did not happen says nothing about coverage."
        )),
    }
}

/// The order a session injects its mutants in, and the buckets it groups them into.
pub(crate) struct Execution {
    /// Indices into the plans.
    pub(crate) order: Vec<usize>,
    /// `package → killer target`, in order, each once.
    pub(crate) buckets: Vec<String>,
}

/// Group plans by the package that owns the mutated file, then by their first killer's
/// target, then catalogue order. Cargo's metadata says which package owns a file; where it
/// cannot be read, every plan is its own package's and the catalogue order stands.
pub(crate) fn bucket_order(plans: &[MutationPlan], root: &Path) -> Execution {
    let packages = crate::metadata::package_dirs(root).unwrap_or_default();
    bucketed(plans, &packages)
}

/// [`bucket_order`] over a given package map. Deterministic: a sort on structured keys.
pub(crate) fn bucketed(plans: &[MutationPlan], packages: &[(String, String)]) -> Execution {
    let key = |p: &MutationPlan| {
        let owner = crate::metadata::owner(&p.file, packages)
            .unwrap_or("")
            .to_owned();
        let target = p
            .killers
            .first()
            .map(|k| k.target.label())
            .unwrap_or_default();
        (owner, target, p.index)
    };
    let mut keyed: Vec<((String, String, usize), usize)> =
        plans.iter().enumerate().map(|(i, p)| (key(p), i)).collect();
    keyed.sort();
    let mut buckets: Vec<String> = Vec::new();
    for ((owner, target, _), _) in &keyed {
        let b = format!(
            "{} → {}",
            if owner.is_empty() { "?" } else { owner },
            if target.is_empty() { "suite" } else { target }
        );
        if buckets.last() != Some(&b) {
            buckets.push(b);
        }
    }
    Execution {
        order: keyed.into_iter().map(|(_, i)| i).collect(),
        buckets,
    }
}

/// How a mutant is to be decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Tier 1 only: decided if its build or a declared killer decides it, deferred if not.
    Direct,
    /// The full law of the campaign's strategy, every tier.
    Full,
}

/// One mutant's instructions.
struct Ask {
    /// The baseline suite's binary count, for a Tier-3 step; 0 before there is one.
    baseline_results: usize,
    mode: Mode,
    /// Verify the declared killers around this injection.
    verify: bool,
}

/// What deciding one mutant produced.
enum One {
    Final(Box<Record>, Vec<KillerCheck>),
    /// Every declared killer passed on it, restored; with its seconds.
    Deferred(Vec<KillerCheck>, f64),
}

/// The mutation baseline of one session: what each declared killer did on the pristine
/// tree, run once. N32-H2.
///
/// Keyed by the tree's digest, the killer's structured selector and the parallelism it ran
/// under — never a display string, never a path — and held only for the session: a tree
/// that digests to anything else has no entry, and the map is dropped when the session
/// ends. It records the pristine result only; a mutant's or a restoration's is never kept.
pub(crate) struct Pristine {
    tree: String,
    parallelism: plan::Parallelism,
    words: BTreeMap<(String, plan::TestSelector, u32, u32), &'static str>,
    /// How many killer runs establishing it took. One per unique killer.
    pub(crate) runs: usize,
}

impl Pristine {
    pub(crate) fn new(tree: &str, parallelism: plan::Parallelism) -> Self {
        Self {
            tree: tree.to_owned(),
            parallelism,
            words: BTreeMap::new(),
            runs: 0,
        }
    }

    fn key(&self, tree: &str, k: &plan::TestSelector) -> (String, plan::TestSelector, u32, u32) {
        (
            tree.to_owned(),
            k.clone(),
            self.parallelism.cargo_jobs,
            self.parallelism.test_threads,
        )
    }

    /// Run each killer once on the pristine tree and record what it did. Returns the
    /// labels of those that did not pass.
    fn establish(&mut self, o: &Options, killers: &BTreeSet<plan::TestSelector>) -> Vec<String> {
        let mut ex = campaign::Real {
            root: &o.root,
            deadlines: o.deadlines,
            parallelism: o.parallelism,
            reset_derived: o.reset_derived,
            logs: None,
            failing: Vec::new(),
            rebuilt: Vec::new(),
        };
        let mut failing = Vec::new();
        for k in killers {
            let w = killer_words(&mut ex, std::slice::from_ref(k))
                .first()
                .copied()
                .unwrap_or("unclear");
            self.record(&self.tree.clone(), k, w);
            if w != "passes" {
                failing.push(format!("{} ({w})", k.label()));
            }
        }
        failing
    }

    pub(crate) fn record(&mut self, tree: &str, k: &plan::TestSelector, word: &'static str) {
        let key = self.key(tree, k);
        self.words.insert(key, word);
        self.runs += 1;
    }

    /// What `k` did on the tree that digests to `tree`, if it ran there this session.
    pub(crate) fn get(&self, tree: &str, k: &plan::TestSelector) -> Option<&'static str> {
        self.words.get(&self.key(tree, k)).copied()
    }

    fn forget(&mut self) {
        self.words.clear();
    }
}

/// Where a finalized verdict goes.
struct Sink<'a> {
    journal: &'a mut Journal,
    problems: &'a mut Vec<String>,
    summary: &'a mut campaign::Summary,
    killer_checks: &'a mut (usize, usize),
    mutant_secs: &'a mut Vec<f64>,
}

impl Sink<'_> {
    fn finish(&mut self, r: &Record, checks: &[KillerCheck], p: &MutationPlan, m: &Mutation) {
        println!("  {} {}  ({})", word(&r.verdict), p.name, r.detail);
        for c in checks {
            println!("{}", c.line());
            self.killer_checks.1 += 1;
            if c.verified() {
                self.killer_checks.0 += 1;
            } else {
                self.problems.push(format!(
                    "{}: {} is not a killer ({})",
                    p.name,
                    c.label,
                    c.states()
                ));
            }
        }
        self.mutant_secs.push(r.secs.total);
        if let Err(e) = self.journal.append(r) {
            self.problems.push(format!(
                "{}: the verdict could not be journaled ({e})",
                p.name
            ));
        }
        if let Some(problem) = problem(&r.verdict, &r.detail, m) {
            self.problems.push(problem);
        }
        self.summary.add(r, false);
    }
}

/// One mutant: inject once, decide, restore, verify, record. `Err` only for a failed
/// restoration, which is fatal to the run.
///
/// With `verify`, the declared killers' three observations are taken around this one
/// injection: the pristine one from the session's [`Pristine`] baseline, the mutant one
/// from the very run the verdict was read from — a killer the verdict did not reach is run
/// once more, on the same mutant — and the restored one after the digest-checked
/// restoration, which `cargo test` rebuilds from.
fn run_one(
    o: &Options,
    p: &MutationPlan,
    m: &Mutation,
    ask: &Ask,
    logs: Option<PathBuf>,
    pristine: &Pristine,
) -> Result<One, String> {
    let t0 = Instant::now();
    let path: PathBuf = o.root.join(&m.file);
    let timing =
        |apply: f64, build: f64, steps: &[campaign::StepRun], restore: f64| campaign::Timing {
            apply,
            build,
            tests: steps.iter().map(|s| s.secs).sum(),
            compile_in_tests: steps.iter().map(|s| s.compile_secs).sum(),
            restore,
            total: t0.elapsed().as_secs_f64(),
        };
    let not_injected = |why: String, original: Option<&str>| -> Result<One, String> {
        let v = Verdict::NotInjected(why);
        let (proof, digest) = match original {
            Some(orig) => (
                campaign::untouched(&path, orig)?,
                campaign::blake(orig.as_bytes()),
            ),
            // Nothing could be read, so nothing was written. The proof is vacuous and
            // says so: the digests are of the empty input.
            None => (campaign::untouched_absent(), campaign::blake(b"")),
        };
        Ok(One::Final(
            Box::new(Record::finalize(
                p,
                o.strategy,
                &v,
                Vec::new(),
                digest,
                None,
                proof,
                timing(0.0, 0.0, &[], 0.0),
                Vec::new(),
            )),
            Vec::new(),
        ))
    };

    let Ok(original) = fs::read_to_string(&path) else {
        return not_injected(format!("cannot read {}", m.file), None);
    };
    let occurrences = original.matches(&m.find).count();
    if occurrences == 0 {
        return not_injected(format!("pattern not found in {}", m.file), Some(&original));
    }
    if occurrences > 1 {
        return not_injected(
            format!(
                "pattern occurs {occurrences} times in {}; it must be unique",
                m.file
            ),
            Some(&original),
        );
    }
    let mutated = original.replacen(&m.find, &m.replace, 1);
    if mutated == original {
        return not_injected(
            "replacement produced an identical file".to_owned(),
            Some(&original),
        );
    }

    let mut ex = campaign::Real {
        root: &o.root,
        deadlines: o.deadlines,
        parallelism: o.parallelism,
        reset_derived: o.reset_derived,
        logs,
        failing: Vec::new(),
        rebuilt: Vec::new(),
    };
    let verify = ask.verify && !p.killers.is_empty();

    // Write-ahead: the original goes to disk before the file is touched, so a killed
    // process leaves a recoverable record rather than a silently mutated tree. The
    // record carries the hash of the *mutated* form too, so recovery never overwrites
    // edits made after a crash.
    let backup = backup_path(&o.root);
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
        return not_injected(
            format!("cannot write the backup for {}", m.file),
            Some(&original),
        );
    }
    if fs::write(&path, &mutated).is_err() {
        let _ = fs::remove_file(&backup);
        return not_injected(format!("cannot write {}", m.file), Some(&original));
    }
    let apply = t0.elapsed().as_secs_f64();

    // The one injection's decision, every test step it ran remembered.
    let (prov, seen) = {
        let mut rec = campaign::Recording {
            inner: &mut ex,
            seen: Vec::new(),
        };
        let prov = match ask.mode {
            Mode::Direct => campaign::decide_direct(p, &mut rec),
            Mode::Full => Some(campaign::decide(
                p,
                o.strategy,
                &mut rec,
                ask.baseline_results,
            )),
        };
        (prov, rec.seen)
    };
    // What the mutant's workspace build compiled, by package: advisory, N32-H3.
    let rebuilt = std::mem::take(&mut ex.rebuilt);
    if !rebuilt.is_empty() {
        let mut by: BTreeMap<&str, usize> = BTreeMap::new();
        for u in &rebuilt {
            *by.entry(u.split(' ').next().unwrap_or("?")).or_insert(0) += 1;
        }
        let parts: Vec<String> = by.iter().map(|(k, n)| format!("{k} {n}")).collect();
        println!(
            "  build        {}: {} unit(s) compiled in {:.1}s ({})",
            p.name,
            rebuilt.len(),
            prov.as_ref().map_or(0.0, |v| v.build_secs),
            parts.join(", ")
        );
    }
    // The mutant half: the verdict's own observation of each killer, or — for one the
    // verdict did not reach — a run on this same mutant.
    let mutant = if verify {
        let failing = std::mem::take(&mut ex.failing);
        let w: Vec<&'static str> = p
            .killers
            .iter()
            .map(|k| match observed_word(&seen, k) {
                Some(w) => w,
                None => killer_words(&mut ex, std::slice::from_ref(k))
                    .first()
                    .copied()
                    .unwrap_or("unclear"),
            })
            .collect();
        ex.failing = failing;
        w
    } else {
        Vec::new()
    };

    let r0 = Instant::now();
    let proof =
        campaign::restore(&path, &original, &backup, |p, c| fs::write(p, c)).map_err(|e| {
            format!(
                "{} was mutated and could not be restored ({e}). Its original is preserved at {} \
             and has deliberately been kept. Recover with `cargo xtask mutate-recover \
             --confirm-no-writers` once nothing is still writing to this tree.",
                m.file,
                backup.display()
            )
        })?;
    let restore_secs = r0.elapsed().as_secs_f64();
    let mut candidates = std::mem::take(&mut ex.failing);
    candidates.dedup();
    // The restored half: the file is its original bytes again, digest-checked above, and
    // `cargo test` rebuilds from them.
    let checks = if verify {
        let restored = killer_words(&mut ex, &p.killers);
        p.killers
            .iter()
            .enumerate()
            .map(|(i, k)| KillerCheck {
                label: k.label(),
                pristine: pristine.get(&pristine.tree, k).unwrap_or("unclear"),
                mutant: mutant.get(i).copied().unwrap_or("unclear"),
                restored: restored.get(i).copied().unwrap_or("unclear"),
            })
            .collect()
    } else {
        Vec::new()
    };
    let Some(prov) = prov else {
        return Ok(One::Deferred(checks, t0.elapsed().as_secs_f64()));
    };
    let secs = timing(apply, prov.build_secs, &prov.steps, restore_secs);
    Ok(One::Final(
        Box::new(Record::finalize(
            p,
            o.strategy,
            &prov.verdict,
            prov.steps,
            campaign::blake(original.as_bytes()),
            Some(campaign::blake(mutated.as_bytes())),
            proof,
            secs,
            candidates,
        )),
        checks,
    ))
}

/// One declared killer's three observations around one injection. Verified only as
/// `passes`, `fails`, `passes` — the law `verify_killer` applies, unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KillerCheck {
    pub(crate) label: String,
    pub(crate) pristine: &'static str,
    pub(crate) mutant: &'static str,
    pub(crate) restored: &'static str,
}

impl KillerCheck {
    pub(crate) fn verified(&self) -> bool {
        self.pristine == "passes" && self.mutant == "fails" && self.restored == "passes"
    }

    fn states(&self) -> String {
        format!(
            "pristine {}, mutant {}, restored {}",
            self.pristine, self.mutant, self.restored
        )
    }

    fn line(&self) -> String {
        format!(
            "    {} {}: {}",
            if self.verified() {
                "VERIFIED    "
            } else {
                "NOT A KILLER"
            },
            self.label,
            self.states()
        )
    }
}

/// What one exact killer run says: `passes`, `fails`, or `unclear` for anything that is
/// not exactly one test reported once.
fn killer_word(obs: &Observation) -> &'static str {
    match obs {
        Observation::Completed {
            passed: 1,
            failing: 0,
            result_lines: 1,
            ..
        } => "passes",
        Observation::Completed {
            passed: 0,
            failing: 1,
            result_lines: 1,
            ..
        } => "fails",
        _ => "unclear",
    }
}

/// What killer `k` was observed doing in `seen`, the steps a verdict ran: its own run and
/// no other killer's, read as a killer run is read. `None` if the verdict did not reach it.
fn observed_word(
    seen: &[(Option<plan::TestSelector>, Observation)],
    k: &plan::TestSelector,
) -> Option<&'static str> {
    seen.iter()
        .find(|(s, _)| s.as_ref() == Some(k))
        .map(|(_, obs)| killer_word(obs))
}

/// Run each killer exactly, as a Tier-1 step is run, and read each.
fn killer_words(ex: &mut campaign::Real<'_>, killers: &[plan::TestSelector]) -> Vec<&'static str> {
    killers
        .iter()
        .map(|k| {
            let step = campaign::Step {
                tier: campaign::Tier::Direct,
                selector: Some(k.clone()),
            };
            killer_word(&ex.test(&step, 1).0)
        })
        .collect()
}

/// The session's timing: setup — tree digest, pristine build, baseline suite, killer
/// listing — once, then each decided mutant. N32-H.
fn session_timing(setup: f64, mutants: &[f64], wall: f64) -> String {
    let mut sorted = mutants.to_vec();
    sorted.sort_by(f64::total_cmp);
    // Nearest rank, in whole percent: no float index.
    let pick = |percent: usize| {
        let rank = (sorted.len().saturating_sub(1) * percent + 50)
            .checked_div(100)
            .unwrap_or(0);
        sorted.get(rank).copied().unwrap_or(0.0)
    };
    format!(
        "  session timing       setup {setup:.0}s once; {} mutant(s) decided, {:.0}s, median {:.1}s, p95 {:.1}s; wall {wall:.0}s",
        mutants.len(),
        mutants.iter().sum::<f64>(),
        pick(50),
        pick(95)
    )
}

/// A killer candidate named on the command line: `package:lib:name`,
/// `package:test=T:name` or `package:bin=B:name`.
pub(crate) fn parse_killer(s: &str) -> Result<KillerSpec, String> {
    let mut parts = s.splitn(3, ':');
    let (Some(package), Some(target), Some(name)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(format!(
            "`{s}`: a killer is `package:lib:name`, `package:test=T:name` or `package:bin=B:name`"
        ));
    };
    let mut k = KillerSpec {
        package: package.to_owned(),
        lib: false,
        test: None,
        bin: None,
        name: name.to_owned(),
    };
    match target.split_once('=') {
        None if target == "lib" => k.lib = true,
        Some(("test", t)) => k.test = Some(t.to_owned()),
        Some(("bin", b)) => k.bin = Some(b.to_owned()),
        _ => {
            return Err(format!(
                "`{s}`: the target must be `lib`, `test=T` or `bin=B`"
            ))
        }
    }
    Ok(k)
}

/// Establish that each candidate killer observes this mutant, the only way a killer is
/// allowed to become catalogue metadata:
///
/// ```text
/// pristine: the test passes → mutant: the same test fails → restored: it passes again
/// ```
///
/// Returns a report and the TOML to paste; errors if any candidate does not qualify.
pub(crate) fn verify_killer(
    o: &Options,
    name: &str,
    given: &[KillerSpec],
) -> Result<String, Vec<String>> {
    use std::fmt::Write as _;
    let cat = plan::load(&o.catalogue).map_err(|e| vec![e])?;
    let Some(index) = cat.mutation.iter().position(|m| m.name == name) else {
        return Err(vec![format!("no mutation is named exactly `{name}`")]);
    };
    let mut m = cat
        .mutation
        .get(index)
        .cloned()
        .ok_or_else(|| vec!["unreachable".to_owned()])?;
    if !given.is_empty() {
        m.killers = given.to_vec();
    }
    if m.killers.is_empty() {
        return Err(vec![format!(
            "`{name}` declares no killer. Name a candidate with `--killer package:test=T:name`."
        )]);
    }
    let mut cat1 = cat.clone();
    if let Some(slot) = cat1.mutation.get_mut(index) {
        *slot = m.clone();
    }
    let plans = plan::plan(&cat1, &[index])?;
    let Some(p) = plans.first() else {
        return Err(vec!["unreachable: no plan".to_owned()]);
    };

    let lock = RunLock::acquire(&o.root).map_err(|e| vec![e.describe(&o.root)])?;
    if backup_path(&o.root).exists() {
        return Err(vec![stranded_refusal(&o.root)]);
    }
    let mut ex = campaign::Real {
        root: &o.root,
        deadlines: o.deadlines,
        parallelism: o.parallelism,
        reset_derived: o.reset_derived,
        logs: None,
        failing: Vec::new(),
        rebuilt: Vec::new(),
    };
    let build = |ex: &mut campaign::Real<'_>, when: &str| -> Result<(), String> {
        match ex.build().0 {
            campaign::Build::Built => Ok(()),
            other => Err(format!("{when}: the workspace did not build ({other:?})")),
        }
    };
    let run_all_killers = |ex: &mut campaign::Real<'_>| -> Vec<(String, &'static str)> {
        p.killers
            .iter()
            .map(|k| {
                let step = campaign::Step {
                    tier: campaign::Tier::Direct,
                    selector: Some(k.clone()),
                };
                let (obs, _, _) = ex.test(&step, 1);
                (k.label(), killer_word(&obs))
            })
            .collect()
    };

    let clock = Instant::now();
    let mut phases: Vec<(&str, f64)> = Vec::new();
    let mark = |phases: &mut Vec<(&'static str, f64)>, what: &'static str, since: &mut Instant| {
        phases.push((what, since.elapsed().as_secs_f64()));
        *since = Instant::now();
    };
    let mut since = Instant::now();
    build(&mut ex, "pristine").map_err(|e| vec![e])?;
    mark(&mut phases, "pristine build", &mut since);
    verify_listed(o, &plans)?;
    mark(&mut phases, "killer listing", &mut since);
    let before = run_all_killers(&mut ex);
    mark(&mut phases, "pristine killers", &mut since);

    let path = o.root.join(&m.file);
    let original = fs::read_to_string(&path).map_err(|e| vec![format!("{}: {e}", m.file)])?;
    if original.matches(&m.find).count() != 1 {
        return Err(vec![format!(
            "`{name}`: the pattern does not occur exactly once in {}",
            m.file
        )]);
    }
    let mutated = original.replacen(&m.find, &m.replace, 1);
    let backup = backup_path(&o.root);
    let record = format!(
        "{RECORD_VERSION}\n{}\n{}\n{}\n{original}",
        m.file,
        digest(&mutated),
        digest(&original)
    );
    fs::write(&backup, &record).map_err(|e| vec![format!("cannot write the backup: {e}")])?;
    if let Err(e) = fs::write(&path, &mutated) {
        let _ = fs::remove_file(&backup);
        return Err(vec![format!("cannot write {}: {e}", m.file)]);
    }
    mark(&mut phases, "inject", &mut since);
    let under = match build(&mut ex, "mutant") {
        Ok(()) => {
            mark(&mut phases, "mutant build", &mut since);
            let r = run_all_killers(&mut ex);
            mark(&mut phases, "mutant killers", &mut since);
            Ok(r)
        }
        Err(e) => Err(e),
    };
    campaign::restore(&path, &original, &backup, |p, c| fs::write(p, c)).map_err(|e| {
        vec![format!(
            "{} could not be restored ({e}); its original is at {}",
            m.file,
            backup.display()
        )]
    })?;
    mark(&mut phases, "restore", &mut since);
    build(&mut ex, "restored").map_err(|e| vec![e])?;
    mark(&mut phases, "restored build", &mut since);
    let after = run_all_killers(&mut ex);
    mark(&mut phases, "restored killers", &mut since);
    drop(lock);
    let timing: Vec<String> = phases.iter().map(|(w, t)| format!("{w} {t:.1}s")).collect();
    println!(
        "timing {name}: {} — total {:.1}s",
        timing.join(", "),
        clock.elapsed().as_secs_f64()
    );
    let under = under.map_err(|e| {
        vec![format!(
            "{e}: a mutant that does not build cannot verify a killer"
        )]
    })?;

    let mut report = String::new();
    let mut failed = Vec::new();
    let mut ok = Vec::new();
    for (i, k) in m.killers.iter().enumerate() {
        let label = before.get(i).map_or("?", |b| b.0.as_str());
        let (b, u, a) = (
            before.get(i).map_or("?", |x| x.1),
            under.get(i).map_or("?", |x| x.1),
            after.get(i).map_or("?", |x| x.1),
        );
        let verified = b == "passes" && u == "fails" && a == "passes";
        let _ = writeln!(
            report,
            "  {} {label}: pristine {b}, mutant {u}, restored {a}",
            if verified {
                "VERIFIED   "
            } else {
                "NOT A KILLER"
            }
        );
        if verified {
            ok.push(k.clone());
        } else {
            failed.push(format!(
                "{name}: {label} is not a killer (pristine {b}, mutant {u}, restored {a})"
            ));
        }
    }
    if !ok.is_empty() {
        let _ = writeln!(report, "\ncatalogue metadata for `{name}`:\nkillers = [");
        for k in &ok {
            let target = if k.lib {
                "lib = true".to_owned()
            } else if let Some(t) = &k.test {
                format!("test = \"{t}\"")
            } else if let Some(b) = &k.bin {
                format!("bin = \"{b}\"")
            } else {
                String::new()
            };
            let _ = writeln!(
                report,
                "    {{ package = \"{}\", {target}, name = \"{}\" }},",
                k.package, k.name
            );
        }
        let _ = writeln!(report, "]");
    }
    if failed.is_empty() {
        Ok(report)
    } else {
        print!("{report}");
        Err(failed)
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use crate::campaign::crash_signature;

    use super::{
        backup_path, digest, legacy_lock_path, lock_path, restore_stranded, AcquireError, Recovery,
        RunLock,
    };
    use std::fs;
    use std::path::Path;
    #[cfg(unix)]
    use std::process::Command;

    fn selector(name: &str) -> crate::plan::TestSelector {
        crate::plan::TestSelector::from_killer(&crate::plan::KillerSpec {
            package: "p".to_owned(),
            lib: false,
            test: Some("t".to_owned()),
            bin: None,
            name: name.to_owned(),
        })
        .unwrap()
    }

    /// A mutant's verification reads each killer's own observation from the verdict's
    /// steps — never another killer's — and runs one the verdict did not reach. N32-H2.
    #[test]
    fn a_killers_mutant_observation_is_its_own_run() {
        use crate::campaign::Observation;
        let done = |passed, failing| Observation::Completed {
            passed,
            failing,
            result_lines: 1,
            crash: None,
        };
        let seen = vec![
            (Some(selector("first")), done(1, 0)),
            (Some(selector("second")), done(0, 1)),
            (None, done(100, 0)),
        ];
        assert_eq!(
            super::observed_word(&seen, &selector("first")),
            Some("passes")
        );
        assert_eq!(
            super::observed_word(&seen, &selector("second")),
            Some("fails")
        );
        assert_eq!(super::observed_word(&seen, &selector("third")), None);
    }

    /// A pristine result is of the tree it was taken on and of nothing else: another tree
    /// digest has no entry, and forgetting the session empties it. N32-H2.
    #[test]
    fn a_pristine_result_is_of_its_tree_and_nothing_else() {
        let mut p = super::Pristine::new("tree-a", crate::plan::Parallelism::SERIAL);
        p.record("tree-a", &selector("k"), "passes");
        assert_eq!(p.get("tree-a", &selector("k")), Some("passes"));
        assert_eq!(p.get("tree-b", &selector("k")), None);
        assert_eq!(p.get("tree-a", &selector("other")), None);
        assert_eq!(p.runs, 1);
        p.forget();
        assert_eq!(p.get("tree-a", &selector("k")), None);
    }

    fn planned(index: usize, file: &str, killer: &str) -> crate::plan::MutationPlan {
        crate::plan::MutationPlan {
            index,
            name: format!("m{index}"),
            file: file.to_owned(),
            killers: vec![selector(killer)],
            profile: None,
        }
    }

    /// Execution order groups by owning package, then killer target, then catalogue order —
    /// deterministically — and only reorders: every plan appears exactly once. N32-H3.
    #[test]
    fn mutants_run_grouped_by_package_then_killer_and_every_one_once() {
        let packages: Vec<(String, String)> = [
            ("docs", "crates/nazm-docs"),
            ("mcp", "crates/nazm-mcp"),
            ("xtask", "xtask"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .to_vec();
        let plans = vec![
            planned(0, "crates/nazm-mcp/src/main.rs", "k"),
            planned(1, "crates/nazm-docs/src/lib.rs", "k"),
            planned(2, "xtask/src/mutate.rs", "k"),
            planned(3, "crates/nazm-mcp/src/main.rs", "k"),
            planned(4, "crates/nazm-docs/src/markdown.rs", "k"),
        ];
        let e = super::bucketed(&plans, &packages);
        assert_eq!(e.order, [1, 4, 0, 3, 2]);
        assert_eq!(e.buckets.len(), 3);
        assert_eq!(super::bucketed(&plans, &packages).order, e.order);
        let mut all = e.order.clone();
        all.sort_unstable();
        assert_eq!(all, [0, 1, 2, 3, 4]);
        // With no package map, the catalogue order stands.
        assert_eq!(super::bucketed(&plans, &[]).order, [0, 1, 2, 3, 4]);
        // The longest directory owns a file.
        let nested = vec![
            ("outer".to_owned(), String::new()),
            ("inner".to_owned(), "crates/a".to_owned()),
        ];
        assert_eq!(
            crate::metadata::owner("crates/a/src/x.rs", &nested),
            Some("inner")
        );
        assert_eq!(crate::metadata::owner("README.md", &nested), Some("outer"));
        assert_eq!(
            crate::metadata::owner("crates/ab/x.rs", nested.get(1..).unwrap()),
            None
        );
    }

    /// The verification law a warm session applies is `verify_killer`'s, and only its
    /// exact shape verifies: passes, fails, passes. N32-H.
    #[test]
    fn a_killer_is_verified_only_by_passes_fails_passes() {
        let check = |pristine, mutant, restored| super::KillerCheck {
            label: "k".to_owned(),
            pristine,
            mutant,
            restored,
        };
        assert!(check("passes", "fails", "passes").verified());
        for (p, m, r) in [
            ("passes", "passes", "passes"),
            ("passes", "unclear", "passes"),
            ("fails", "fails", "passes"),
            ("unclear", "fails", "passes"),
            ("passes", "fails", "fails"),
            ("passes", "fails", "unclear"),
        ] {
            assert!(!check(p, m, r).verified(), "{p} {m} {r}");
        }
    }

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
        let (root, src) = stranded("original\n", "mutated\n");
        let backup = backup_path(root.path());

        let outcome = crate::campaign::restore(&src, "original\n", &backup, |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected at the restoration boundary",
            ))
        });
        // No proof of restoration, so no record can be built and no verdict finalized.
        let why = outcome.expect_err("a failed restore must not produce a proof");
        assert!(why.contains("could not write the original back"), "{why}");

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
        // The other half, so the test above is not satisfied by a restore that never
        // clears anything.
        let (root, src) = stranded("original\n", "mutated\n");
        let backup = backup_path(root.path());

        crate::campaign::restore(&src, "original\n", &backup, |p, c| fs::write(p, c))
            .expect("a proof of restoration");
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
    #[cfg(unix)] // reads a file another handle holds locked: Windows enforces the lock
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
    #[cfg(unix)] // `ps`; campaigns run contained, on Linux
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
    #[cfg(unix)] // `ps`; campaigns run contained, on Linux
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
    #[cfg(unix)] // `ps`; campaigns run contained, on Linux
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
    #[cfg(unix)] // `ps`; campaigns run contained, on Linux
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
