//! The contained runner: the only path by which a generated compiler stage, the selfhost
//! suite, the bootstrap, fuzzing, the mutation catalogue or a heavy benchmark may run.
//!
//! # Why this exists rather than an environment variable
//!
//! `NAZM_CONTAINED=1` is a *declaration*. This is the ceiling. On 2026-09-21 the selfhost
//! suite panicked the machine three times: the stage drivers accumulated their output by
//! copying, `cargo test` runs one thread per core, and ten of them grew until nothing
//! could be scheduled. macOS could not have stopped it — `RLIMIT_AS` is an alias of
//! `RLIMIT_RSS` there and `setrlimit(2)` documents it as advisory, so the kernel prefers
//! to reclaim from an offender rather than refuse its allocation. A cgroup refuses.
//!
//! The runner sets `NAZM_CONTAINED=1` *inside* the container, where a real ceiling
//! applies. Nothing sets it on the host.
//!
//! # What is bounded, and how each was verified on this daemon
//!
//! | bound | mechanism | verified by |
//! | --- | --- | --- |
//! | memory | `--memory` | probe allocates *and touches* past the cap, `OOMKilled` observed |
//! | swap | `--memory-swap` equal to `--memory` | `memory.swap.max` reads 0 inside |
//! | CPU | `--cpus` | `cpu.max` inside |
//! | processes | `--pids-limit` | `pids.max` inside |
//! | disk | `--storage-opt size` | probe fills the writable layer |
//! | scratch | `tmpfs` with `size=` | probe overruns the tmpfs |
//! | single file | `--ulimit fsize` | probe writes past it |
//! | wall clock | deadline, then `docker kill` on the **container** | probe sleeps past it |
//! | output | concurrent drain with a byte cap | probe floods stdout and stderr |
//! | descendants | `--init` plus container teardown | probe detaches a grandchild |
//!
//! Killing the `docker` client is not killing the container, so the deadline kills the
//! container by name. The flag semantics follow `crates/nazm-bench/src/engine/docker.rs`,
//! which already documents why `--memory-swap` must equal `--memory`.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) const IMAGE: &str = "nazm-contained:1.98.1";

/// Every limit the runner imposes. Defaults are budgets, not guesses: the migrated stages
/// peak near 11 MB, and `rustc` building this workspace needs on the order of 1-2 GB, so
/// 4 GiB leaves room for the build while staying well under the 7.75 GiB Linux VM.
/// Raising one of these to make a workload pass is the wrong move — investigate instead.
#[derive(Debug, Clone)]
pub(crate) struct Bounds {
    pub(crate) memory_mb: u32,
    pub(crate) milli_cpus: u32,
    pub(crate) pids: u32,
    pub(crate) storage_gb: u32,
    pub(crate) scratch_mb: u32,
    pub(crate) fsize_bytes: u64,
    pub(crate) deadline: Duration,
    pub(crate) output_cap: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            memory_mb: 4096,
            milli_cpus: 1000,
            pids: 512,
            storage_gb: 8,
            scratch_mb: 512,
            fsize_bytes: 512 * 1024 * 1024,
            deadline: Duration::from_secs(3600),
            output_cap: 4 * 1024 * 1024,
        }
    }
}

/// Distinct outcomes. Collapsing these into "failed" is what lets a resource problem be
/// mistaken for a test problem, so they never share a variant.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Passed,
    Failed(i32),
    Timeout,
    MemoryLimit,
    OutputOverflow,
    ToolFailure(String),
    Unavailable(String),
}

impl Outcome {
    pub(crate) fn word(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed(_) => "failed",
            Self::Timeout => "timeout",
            Self::MemoryLimit => "memory-limit",
            Self::OutputOverflow => "output-overflow",
            Self::ToolFailure(_) => "tool-failure",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

pub(crate) struct Report {
    pub(crate) outcome: Outcome,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_bytes: u64,
    pub(crate) stderr_bytes: u64,
    pub(crate) elapsed: Duration,
    pub(crate) oom_killed: bool,
}

fn docker_available() -> Result<(), String> {
    match Command::new("docker").arg("info").output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "the Docker daemon is not reachable: {}",
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
        )),
        Err(e) => Err(format!("could not run `docker`: {e}")),
    }
}

fn image_present() -> bool {
    Command::new("docker")
        .args(["image", "inspect", IMAGE])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Drains a stream to exhaustion on its own thread, keeping at most `cap` bytes. Draining
/// matters as much as capping: a child that fills a pipe nobody reads simply blocks, and
/// the deadline then reports a hang that is really a full buffer.
fn drain<R: Read + Send + 'static>(mut r: R, cap: usize) -> mpsc::Receiver<(Vec<u8>, u64)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut total: u64 = 0;
        let mut buf = [0u8; 64 * 1024];
        while let Ok(n) = r.read(&mut buf) {
            if n == 0 {
                break;
            }
            total += n as u64;
            if kept.len() < cap {
                let room = cap - kept.len();
                if let Some(slice) = buf.get(..n.min(room)) {
                    kept.extend_from_slice(slice);
                }
            }
        }
        let _ = tx.send((kept, total));
    });
    rx
}

/// Runs `argv` inside the bounded environment with `repo` mounted read-only at `/src`.
pub(crate) fn run(repo: &Path, name: &str, argv: &[&str], b: &Bounds) -> Report {
    let empty = Report {
        outcome: Outcome::Passed,
        stdout: String::new(),
        stderr: String::new(),
        stdout_bytes: 0,
        stderr_bytes: 0,
        elapsed: Duration::ZERO,
        oom_killed: false,
    };
    if let Err(why) = docker_available() {
        return Report {
            outcome: Outcome::Unavailable(why),
            ..empty
        };
    }
    if !image_present() {
        return Report {
            outcome: Outcome::Unavailable(format!(
                "image `{IMAGE}` is absent. Build it:\n    docker build -f docker/contained.Dockerfile -t {IMAGE} ."
            )),
            ..empty
        };
    }

    let container = format!("nazm-{name}-{}", std::process::id());
    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm", "--name", &container])
        .args(["--memory", &format!("{}m", b.memory_mb)])
        // Equal to --memory: a cap that can be exceeded by swapping is not a cap.
        .args(["--memory-swap", &format!("{}m", b.memory_mb)])
        .args([
            "--cpus",
            &format!("{:.3}", f64::from(b.milli_cpus) / 1000.0),
        ])
        .args(["--pids-limit", &b.pids.to_string()])
        .args(["--storage-opt", &format!("size={}G", b.storage_gb)])
        .args(["--ulimit", &format!("fsize={}", b.fsize_bytes)])
        .args([
            "--tmpfs",
            &format!("/scratch:rw,exec,size={}m", b.scratch_mb),
        ])
        // Reaps orphans, so a detached grandchild cannot accumulate against the pid limit.
        .arg("--init")
        .args(["--log-driver", "none"])
        .arg("--mount")
        .arg(format!(
            "type=bind,source={},target=/src,readonly",
            repo.display()
        ))
        // The only place a real ceiling applies, so the only place this is set.
        .args(["--env", "NAZM_CONTAINED=1"])
        .args(["--env", "CARGO_TARGET_DIR=/work/target"])
        .args(["--env", "CARGO_HOME=/work/cargo"])
        .arg(IMAGE)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Report {
                outcome: Outcome::ToolFailure(format!("spawn: {e}")),
                ..empty
            };
        }
    };
    let (Some(cout), Some(cerr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Report {
            outcome: Outcome::ToolFailure("docker gave no pipes to drain".to_owned()),
            ..empty
        };
    };
    let out_rx = drain(cout, b.output_cap);
    let err_rx = drain(cerr, b.output_cap);

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if started.elapsed() >= b.deadline {
                    timed_out = true;
                    // The container, not the client: terminating the client leaves the
                    // daemon's container running.
                    let _ = Command::new("docker").args(["kill", &container]).output();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Report {
                    outcome: Outcome::ToolFailure(format!("wait: {e}")),
                    ..empty
                };
            }
        }
    };
    let elapsed = started.elapsed();
    let (okept, ototal) = out_rx.recv().unwrap_or_default();
    let (ekept, etotal) = err_rx.recv().unwrap_or_default();

    // `--rm` removes it, so inspect may legitimately find nothing; absence is not evidence
    // of an OOM either way, and the exit code carries the signal.
    let oom = Command::new("docker")
        .args(["inspect", &container, "--format", "{{.State.OOMKilled}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);
    let _ = Command::new("docker")
        .args(["rm", "-f", &container])
        .output();

    let code = status.and_then(|s| s.code()).unwrap_or(-1);
    let cap = b.output_cap as u64;
    let overflow = ototal > cap || etotal > cap;
    let outcome = if timed_out {
        Outcome::Timeout
    } else if oom || code == 137 {
        Outcome::MemoryLimit
    } else if code == 0 {
        if overflow {
            Outcome::OutputOverflow
        } else {
            Outcome::Passed
        }
    } else {
        Outcome::Failed(code)
    };

    Report {
        outcome,
        stdout: String::from_utf8_lossy(&okept).into_owned(),
        stderr: String::from_utf8_lossy(&ekept).into_owned(),
        stdout_bytes: ototal,
        stderr_bytes: etotal,
        elapsed,
        oom_killed: oom,
    }
}

/// Containers this runner left behind. Always zero, or the runner has a leak.
pub(crate) fn survivors() -> usize {
    Command::new("docker")
        .args(["ps", "-aq", "--filter", "name=nazm-"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}
