//! The one way the mutation harness runs a subprocess.
//!
//! Every build, every targeted test, every focused profile and the broad suite go through
//! [`run`]. There is deliberately no second implementation: N12.1 showed that subprocess
//! handling is where a harness quietly stops telling the truth. A test deadline there
//! killed its direct child and left the compiled program it had spawned running, the pipe
//! stayed open, the reader never returned, and three campaigns ended with no verdict.
//!
//! # What an invocation owns, and how it is ended
//!
//! * **Its own process group.** The child is its group's leader (`process_group(0)`), so
//!   the group id is the child's pid and cannot be this process's group. Everything the
//!   child starts stays in that group unless it asks to leave.
//! * **Its descendants, found by parentage.** A test that bounds a compiled program puts
//!   that program in *another* group (the N12.1 fix did exactly this), so the group
//!   alone is not the tree. At a deadline the tree is walked from the child through
//!   parent ids and every process found is **stopped** before anything is killed: a
//!   stopped process cannot fork, exit or be re-parented, so repeating the walk until it
//!   finds nothing new yields the whole tree. Then every one of them is killed.
//! * **A watchdog, in a group of its own.** If this process dies (SIGINT, SIGKILL, a
//!   panic), nothing here survives to clean up. So each invocation also starts a
//!   `sh` that ignores the terminal's signals and blocks reading a pipe that only this
//!   process holds the write end of. The kernel closes that end when this process dies,
//!   the read returns, and the watchdog kills the invocation's group. On a normal finish
//!   the watchdog is killed by pid first. It is this process's own unreaped child, so
//!   that pid cannot have been reused.
//!
//! # What this does not cover, stated rather than implied away
//!
//! A descendant that leaves the group **and** is re-parented before a deadline walks the
//! tree (its parent exited first) is invisible to both mechanisms. Inside
//! `cargo xtask contained mutate` that residue dies with the container's pid namespace.
//! Outside it, it is why recovery after an interruption stays an operator's attested
//! action (`docs/internal/runbook.md`).
//!
//! # Every id is validated before it is signalled
//!
//! Nothing here signals pid 0, pid 1, a negative number it did not construct, this
//! process, or a group other than one this process created. There is no `pkill`, no
//! match on a command line, and no `kill(0, …)`.
//!
//! # On Windows (W1)
//!
//! There are no process groups, no stopped processes and no `sh` watchdog: at a deadline the
//! child's tree is ended by `taskkill /T /F` (parentage, as the walk above), and nothing is
//! swept after a leader that has exited. Mutation campaigns run contained, on Linux; on
//! Windows this is what the host-safe suite's deadlines need.

use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How much of each stream is kept in memory. Beyond this the invocation is reported as
/// having overflowed. That is an infrastructure outcome, never a verdict: a mutant that
/// floods its output has not been shown to be caught by anything.
pub(crate) const OUTPUT_CAP: usize = 32 * 1024 * 1024;

/// How long a stream may stay open after its process has exited, before the group is
/// swept and, if that does not close it, the invocation is reported as holding a pipe.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// One subprocess to run.
#[derive(Debug, Clone)]
pub(crate) struct Invocation {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: std::path::PathBuf,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) deadline: Duration,
}

impl Invocation {
    /// A human-readable rendering, for logs. Never executed.
    pub(crate) fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

/// How the process ended. A **process** outcome, and nothing more: what it means for a
/// mutation is decided elsewhere, from the tier it ran at and the rules that tier obeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It exited or was signalled on its own, and its output streams closed.
    Exited(ExitStatus),
    /// The deadline passed. The tree was stopped and killed.
    TimedOut,
    /// It exited, but a stream stayed open even after its group was swept. Something
    /// outside the group still holds the pipe.
    PipeHeld,
    /// More than [`OUTPUT_CAP`] bytes on a stream.
    Overflowed,
    /// It could not be started at all.
    SpawnFailed(String),
}

#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) ended: Ended,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) elapsed: Duration,
    /// Processes that were still in the group after the leader exited, and were killed.
    /// Zero in the ordinary case. It is reported so a test leaking children is visible.
    pub(crate) swept: usize,
}

impl Finished {
    fn failed_to_start(why: String) -> Self {
        Self {
            ended: Ended::SpawnFailed(why),
            stdout: String::new(),
            stderr: String::new(),
            elapsed: Duration::ZERO,
            swept: 0,
        }
    }
}

/// The signal that ended a process, if one did; never on Windows, which has none.
pub(crate) fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Run one invocation to completion, deadline, or cleanup.
pub(crate) fn run(inv: &Invocation) -> Finished {
    let mut cmd = Command::new(&inv.program);
    cmd.args(&inv.args)
        .current_dir(&inv.cwd)
        .envs(inv.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }

    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Finished::failed_to_start(format!("{}: {e}", inv.program)),
    };
    let leader = child.id();
    // The watchdog is armed before anything else can fail, and every exit path below
    // disarms it.
    let watchdog = Watchdog::arm(leader);

    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        kill_tree(leader);
        let _ = child.wait();
        drop(watchdog);
        return Finished::failed_to_start("the child gave no pipes to drain".to_owned());
    };
    let out_rx = drain(out);
    let err_rx = drain(err);

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= inv.deadline => {
                timed_out = true;
                kill_tree(leader);
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                kill_tree(leader);
                break child.wait().ok();
            }
        }
    };

    // The leader is reaped. Its streams close when every holder has gone, which is not
    // the same moment: a child it left behind may still have them.
    let mut stdout = out_rx.recv_timeout(DRAIN_GRACE).ok();
    let mut stderr = err_rx.recv_timeout(DRAIN_GRACE).ok();
    // Swept either way. With a stream still open, the sweep is what closes it; with both
    // closed, a member that closed its copies may still be running, and it is this
    // invocation's and does not outlive it.
    let swept = sweep_group(leader);
    if stdout.is_none() {
        stdout = out_rx.recv_timeout(DRAIN_GRACE).ok();
    }
    if stderr.is_none() {
        stderr = err_rx.recv_timeout(DRAIN_GRACE).ok();
    }
    drop(watchdog);
    let elapsed = started.elapsed();

    let held = stdout.is_none() || stderr.is_none();
    let (out_bytes, out_over) = stdout.unwrap_or_default();
    let (err_bytes, err_over) = stderr.unwrap_or_default();
    let ended = if timed_out {
        Ended::TimedOut
    } else if held {
        Ended::PipeHeld
    } else if out_over || err_over {
        Ended::Overflowed
    } else {
        match status {
            Some(s) => Ended::Exited(s),
            None => Ended::SpawnFailed("the child could not be waited for".to_owned()),
        }
    };
    Finished {
        ended,
        stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
        elapsed,
        swept,
    }
}

/// Read a stream to its end, keeping at most [`OUTPUT_CAP`] bytes. Sends once, at EOF.
fn drain<R: Read + Send + 'static>(mut r: R) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut over = false;
        let mut buf = [0u8; 64 * 1024];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = OUTPUT_CAP.saturating_sub(kept.len());
                    if n > room {
                        over = true;
                    }
                    kept.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
                }
            }
        }
        let _ = tx.send((kept, over));
    });
    rx
}

/// Kills its invocation's group if this process dies first.
struct Watchdog {
    child: Option<Child>,
    /// Held, never written. Its closing is the signal.
    _pipe: Option<ChildStdin>,
}

impl Watchdog {
    #[cfg(not(unix))]
    fn arm(_leader: u32) -> Self {
        Self {
            child: None,
            _pipe: None,
        }
    }

    #[cfg(unix)]
    fn arm(leader: u32) -> Self {
        use std::os::unix::process::CommandExt as _;
        if !valid_leader(leader) {
            return Self {
                child: None,
                _pipe: None,
            };
        }
        // `$1` is the group, passed as an argument rather than interpolated into the
        // script, so the script text is a constant.
        //
        // `read` returns only at end of file: nothing ever writes to this pipe. It
        // reaches end of file when the last write end closes, and the only write end is
        // held by this process, which Rust opens close-on-exec, so no other child
        // inherits it.
        let spawned = Command::new("sh")
            .args([
                "-c",
                "trap '' INT TERM HUP QUIT; read _ignored; exec env kill -KILL -- \"-$1\"",
                "nazm-mutation-watchdog",
                &leader.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Its own group, so a signal aimed at *this* process's group (a terminal's
            // ^C, a test killing the runner's group) does not take the watchdog with it.
            .process_group(0)
            .spawn();
        match spawned {
            Ok(mut c) => {
                let pipe = c.stdin.take();
                Self {
                    child: Some(c),
                    _pipe: pipe,
                }
            }
            Err(_) => Self {
                child: None,
                _pipe: None,
            },
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Killed by pid before the pipe closes, so a finished invocation never triggers
        // it. The watchdog is this process's child and not yet reaped, so the pid is
        // still the watchdog's.
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// A group id this module created: a positive pid that is neither `init` nor us.
fn valid_leader(pid: u32) -> bool {
    pid > 1 && pid != std::process::id()
}

/// One row of the process table: pid, parent pid, process group.
type Row = (u32, u32, u32);

fn process_table() -> Vec<Row> {
    let Ok(out) = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace().map(|n| n.parse::<u32>().ok());
            Some((f.next()??, f.next()??, f.next()??))
        })
        .collect()
}

/// The leader, everything descended from it by parent id, and everything in its group.
fn owned(leader: u32, table: &[Row]) -> BTreeSet<u32> {
    let mut set = BTreeSet::from([leader]);
    loop {
        let before = set.len();
        for &(pid, ppid, pgid) in table {
            if set.contains(&ppid) || pgid == leader {
                set.insert(pid);
            }
        }
        if set.len() == before {
            break;
        }
    }
    let me = std::process::id();
    set.retain(|&p| p > 1 && p != me);
    set
}

fn signal(sig: &str, pids: &BTreeSet<u32>) {
    if pids.is_empty() {
        return;
    }
    let mut cmd = Command::new("kill");
    cmd.arg(format!("-{sig}"));
    for p in pids {
        cmd.arg(p.to_string());
    }
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn signal_group(sig: &str, leader: u32) {
    if !valid_leader(leader) {
        return;
    }
    // `--` before the negative id. procps `kill` (the container's) reads `-123` after a
    // signal as another option and fails to parse it, so without `--` the group is never
    // signalled there. BSD `kill` (macOS) accepts both spellings.
    let _ = Command::new("kill")
        .args([format!("-{sig}"), "--".to_owned(), format!("-{leader}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Stop the whole tree, then kill it. Returns how many processes were found.
///
/// Stopping first is what makes the walk complete. A running process can fork after the
/// table was read, or exit and have its children re-parented out of reach. A stopped
/// one can do neither, so the walk is repeated until it finds nothing new.
pub(crate) fn kill_tree(leader: u32) -> usize {
    if !valid_leader(leader) {
        return 0;
    }
    if cfg!(windows) {
        let ended = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &leader.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return usize::from(ended.is_ok_and(|s| s.success()));
    }
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for _ in 0..32 {
        let now = owned(leader, &process_table());
        let fresh: BTreeSet<u32> = now.difference(&seen).copied().collect();
        if fresh.is_empty() {
            break;
        }
        signal("STOP", &fresh);
        seen.extend(fresh);
    }
    signal("KILL", &seen);
    signal_group("KILL", leader);
    seen.len()
}

/// Kill whatever is still in the group once its leader has exited.
fn sweep_group(leader: u32) -> usize {
    if !valid_leader(leader) || cfg!(windows) {
        return 0;
    }
    let members: BTreeSet<u32> = process_table()
        .into_iter()
        .filter(|&(pid, _, pgid)| pgid == leader && pid != leader)
        .map(|(pid, _, _)| pid)
        .collect();
    if members.is_empty() {
        return 0;
    }
    // A member may have children of its own outside the group. Walk from each.
    let mut all = BTreeSet::new();
    for m in &members {
        let table = process_table();
        all.extend(owned(*m, &table));
    }
    signal("STOP", &all);
    signal("KILL", &all);
    signal_group("KILL", leader);
    all.len()
}

/// Is this pid alive? For tests and diagnostics; never used to decide ownership.
#[cfg(all(test, unix))]
pub(crate) fn alive(pid: u32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let s = s.trim();
            // A zombie has exited. It is waiting to be reaped, not running.
            !s.is_empty() && !s.starts_with('Z')
        })
        .unwrap_or(false)
}

#[cfg(all(test, unix))] // `sh`, `ps`, process groups and signals
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::{alive, run, Ended, Invocation};
    use std::time::{Duration, Instant};

    fn sh(script: &str, deadline: Duration, dir: &std::path::Path) -> super::Finished {
        run(&Invocation {
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), script.to_owned()],
            cwd: dir.to_path_buf(),
            env: Vec::new(),
            deadline,
        })
    }

    fn pids_in(dir: &std::path::Path, name: &str) -> Vec<u32> {
        std::fs::read_to_string(dir.join(name))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect()
    }

    fn wait_dead(pids: &[u32]) {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if pids.iter().all(|p| !alive(*p)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let still: Vec<&u32> = pids.iter().filter(|p| alive(**p)).collect();
        panic!("still running after cleanup: {still:?}");
    }

    #[test]
    fn a_child_that_finishes_is_reported_as_it_exited() {
        let dir = tempfile::tempdir().unwrap();
        let r = sh(
            "echo out; echo err >&2; exit 3",
            Duration::from_secs(30),
            dir.path(),
        );
        match r.ended {
            Ended::Exited(s) => assert_eq!(s.code(), Some(3)),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.stdout, "out\n");
        assert_eq!(r.stderr, "err\n");
        assert_eq!(r.swept, 0);
    }

    #[test]
    fn a_child_past_its_deadline_is_killed_and_reported_as_timed_out() {
        let dir = tempfile::tempdir().unwrap();
        let r = sh(
            "echo $$ > child.pid; sleep 60",
            Duration::from_millis(500),
            dir.path(),
        );
        assert_eq!(r.ended, Ended::TimedOut);
        assert!(r.elapsed < Duration::from_secs(20), "{:?}", r.elapsed);
        wait_dead(&pids_in(dir.path(), "child.pid"));
    }

    #[test]
    fn a_grandchild_is_killed_with_its_parent_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let r = sh(
            "sleep 60 & echo $! > grandchild.pid; wait",
            Duration::from_millis(500),
            dir.path(),
        );
        assert_eq!(r.ended, Ended::TimedOut);
        wait_dead(&pids_in(dir.path(), "grandchild.pid"));
    }

    #[test]
    fn a_grandchild_in_a_group_of_its_own_is_found_by_parentage() {
        // The N12.1 shape: a test puts the program it bounds into a separate process
        // group, so killing the test's group misses it. `perl`'s `setpgrp` is the
        // portable way to leave a group from a shell: dash (the container's `sh`) has
        // no job control without a terminal, so `set -m` there creates nothing.
        let dir = tempfile::tempdir().unwrap();
        let r = sh(
            "perl -e 'setpgrp; exec @ARGV' sleep 60 & echo $! > escaped.pid; wait",
            Duration::from_millis(500),
            dir.path(),
        );
        assert_eq!(r.ended, Ended::TimedOut);
        let pids = pids_in(dir.path(), "escaped.pid");
        wait_dead(&pids);
    }

    #[test]
    fn a_group_is_signalled_in_the_spelling_both_kills_accept() {
        // procps `kill -KILL -123` fails to parse and signals nothing; `-- -123` works
        // there and on BSD. The N12.1 test deadlines had the first spelling, which is
        // how this was found. Asserted on a real group, so the spelling is what is
        // tested and not a string.
        use std::os::unix::process::CommandExt as _;
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .unwrap();
        super::signal_group("KILL", child.id());
        let status = child.wait().unwrap();
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(
            status.signal(),
            Some(9),
            "the group was not signalled: {status:?}"
        );
    }

    #[test]
    fn a_grandchild_holding_the_output_pipe_does_not_hang_the_reader() {
        // The parent exits at once. The grandchild keeps stdout open and would keep the
        // reader waiting for an end of file that never comes. It is in the group, so
        // the sweep reaches it, and the invocation still ends.
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let r = sh(
            "sleep 60 & echo $! > holder.pid; exit 0",
            Duration::from_secs(60),
            dir.path(),
        );
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the reader hung"
        );
        assert!(
            matches!(r.ended, Ended::Exited(_)),
            "the leader did exit on its own: {:?}",
            r.ended
        );
        assert!(r.swept >= 1, "the holder was not swept");
        wait_dead(&pids_in(dir.path(), "holder.pid"));
    }

    #[test]
    fn a_grandchild_that_outlives_its_parent_without_the_pipe_is_swept() {
        let dir = tempfile::tempdir().unwrap();
        let r = sh(
            "sleep 60 >/dev/null 2>&1 & echo $! > orphan.pid; exit 0",
            Duration::from_secs(60),
            dir.path(),
        );
        assert!(matches!(r.ended, Ended::Exited(_)), "{:?}", r.ended);
        assert_eq!(r.swept, 1);
        wait_dead(&pids_in(dir.path(), "orphan.pid"));
    }

    #[test]
    fn a_program_that_is_not_there_is_a_spawn_failure_not_an_exit() {
        let dir = tempfile::tempdir().unwrap();
        let r = run(&Invocation {
            program: "/nonexistent/nazm-no-such-program".to_owned(),
            args: Vec::new(),
            cwd: dir.path().to_path_buf(),
            env: Vec::new(),
            deadline: Duration::from_secs(5),
        });
        assert!(matches!(r.ended, Ended::SpawnFailed(_)), "{:?}", r.ended);
    }

    #[test]
    fn the_leader_validation_refuses_what_this_module_did_not_create() {
        assert!(!super::valid_leader(0));
        assert!(!super::valid_leader(1));
        assert!(!super::valid_leader(std::process::id()));
        assert_eq!(super::kill_tree(0), 0);
        assert_eq!(super::kill_tree(1), 0);
        assert_eq!(super::kill_tree(std::process::id()), 0);
    }
}
