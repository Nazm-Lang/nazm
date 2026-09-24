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

/// Build and run the workspace suite on the pristine tree. Returns how many binaries
/// reported, which every later workspace-suite run must match.
fn baseline(o: &Options, identity_tree: &str) -> Result<usize, String> {
    let build = procs::run(&Invocation {
        program: campaign::cargo(),
        args: plan::workspace_build(),
        cwd: o.root.clone(),
        env: Vec::new(),
        deadline: Duration::from_secs(o.deadlines.build_secs),
    });
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
    if o.reset_derived {
        campaign::reset_derived(&o.root);
    }
    let suite = procs::run(&Invocation {
        program: campaign::cargo(),
        args: plan::broad_suite(),
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
                args: target.list_args(ignored),
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
                 selection or deadline set has a different key and is never read.",
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
    let mut baseline_results = 0;
    if pending > 0 {
        baseline_results = match baseline(o, &identity.tree) {
            Ok(n) => n,
            Err(e) => return End::Refused(vec![e]),
        };
        if o.strategy == Strategy::Targeted {
            if let Err(e) = verify_listed(o, &plans) {
                return End::Refused(e);
            }
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
    for p in &plans {
        if let Some(r) = reuse(&p.name) {
            println!(
                "  {} {}  (reused from journal {key}: {})",
                word(&r.verdict),
                p.name,
                r.detail
            );
            summary.add(r, true);
            finalized.insert(p.name.clone());
            if let Some(m) = cat.mutation.get(p.index) {
                if let Some(problem) = problem(&r.verdict, &r.detail, m) {
                    problems.push(problem);
                }
            }
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
        match run_one(o, p, m, baseline_results, Some(logs)) {
            Ok(r) => {
                println!("  {} {}  ({})", word(&r.verdict), p.name, r.detail);
                if let Err(e) = journal.append(&r) {
                    problems.push(format!(
                        "{}: the verdict could not be journaled ({e})",
                        p.name
                    ));
                }
                if let Some(problem) = problem(&r.verdict, &r.detail, m) {
                    problems.push(problem);
                }
                summary.add(&r, false);
                finalized.insert(p.name.clone());
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

    // Every mutation has been injected and restored by now. Released explicitly so the
    // extent of the critical section is visible here rather than inferred from scope.
    drop(run_lock);

    summary.not_reached = not_reached.clone();
    summary.wall = started.elapsed().as_secs_f64();
    print!("{}", summary.render(&key, o.strategy));
    let names: Vec<String> = plans.iter().map(|p| p.name.clone()).collect();
    if !campaign::accounted(&names, &finalized, &not_reached) {
        problems.push(
            "internal: the campaign's accounting does not cover every selected mutation".to_owned(),
        );
    }

    if stopped {
        End::Refused(problems)
    } else if !not_reached.is_empty() {
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

/// One mutant: inject, decide, restore, verify, record. `Err` only for a failed
/// restoration, which is fatal to the run.
fn run_one(
    o: &Options,
    p: &MutationPlan,
    m: &Mutation,
    baseline_results: usize,
    logs: Option<PathBuf>,
) -> Result<Record, String> {
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
    let not_injected = |why: String, original: Option<&str>| -> Result<Record, String> {
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
        Ok(Record::finalize(
            p,
            o.strategy,
            &v,
            Vec::new(),
            digest,
            None,
            proof,
            timing(0.0, 0.0, &[], 0.0),
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

    let mut ex = campaign::Real {
        root: &o.root,
        deadlines: o.deadlines,
        reset_derived: o.reset_derived,
        logs,
        failing: Vec::new(),
    };
    let prov = campaign::decide(p, o.strategy, &mut ex, baseline_results);

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
    let secs = timing(apply, prov.build_secs, &prov.steps, restore_secs);
    let mut candidates = ex.failing;
    candidates.dedup();
    Ok(Record::finalize(
        p,
        o.strategy,
        &prov.verdict,
        prov.steps,
        campaign::blake(original.as_bytes()),
        Some(campaign::blake(mutated.as_bytes())),
        proof,
        secs,
        candidates,
    ))
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
        reset_derived: o.reset_derived,
        logs: None,
        failing: Vec::new(),
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
                let w = match obs {
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
                };
                (k.label(), w)
            })
            .collect()
    };

    build(&mut ex, "pristine").map_err(|e| vec![e])?;
    verify_listed(o, &plans)?;
    let before = run_all_killers(&mut ex);

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
    let under = match build(&mut ex, "mutant") {
        Ok(()) => Ok(run_all_killers(&mut ex)),
        Err(e) => Err(e),
    };
    campaign::restore(&path, &original, &backup, |p, c| fs::write(p, c)).map_err(|e| {
        vec![format!(
            "{} could not be restored ({e}); its original is at {}",
            m.file,
            backup.display()
        )]
    })?;
    build(&mut ex, "restored").map_err(|e| vec![e])?;
    let after = run_all_killers(&mut ex);
    drop(lock);
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
