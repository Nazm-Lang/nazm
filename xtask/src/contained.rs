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
    /// What, if anything, may be copied out when the workload succeeds.
    ///
    /// `None` for every workload that produces nothing. The manifest inside is written by
    /// the workload and is therefore **data, not instruction**: it can say anything, and
    /// the supervisor decides what is permitted.
    pub(crate) export: Option<ExportPolicy>,
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
            export: None,
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
    ExportRefused(String),
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
            Self::ExportRefused(_) => "export-refused",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

/// Where artefacts may come from, where they may go, and which names are allowed.
#[derive(Debug, Clone)]
pub(crate) struct ExportPolicy {
    /// The manifest's path inside the container.
    pub(crate) manifest: String,
    /// The directory validated artefacts are moved into.
    pub(crate) dest: std::path::PathBuf,
    /// Exactly the names permitted. Decided by the supervisor from the mode it was asked
    /// for and the platform it independently measured — never from the manifest, which is
    /// the workload's own output.
    pub(crate) allowed: Vec<String>,
    /// The most any one artefact of this policy may be, in bytes.
    ///
    /// Stated per policy rather than as one constant, because the budgets are genuinely
    /// different and a single number would have to be the largest of them: a benchmark
    /// record is a few kilobytes and a release archive is tens of megabytes, and giving
    /// the record the archive's allowance would mean it could fill a disk unnoticed.
    /// Each caller writes down what its own artefact can legitimately be.
    pub(crate) max_bytes: u64,
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
    // `--rm` only when nothing has to be copied out: a removed container has no files.
    if b.export.is_none() {
        cmd.arg("run").arg("--rm");
    } else {
        cmd.arg("run");
    }
    cmd.args(["--name", &container])
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
    let code = status.and_then(|s| s.code()).unwrap_or(-1);
    let cap = b.output_cap as u64;
    let overflow = ototal > cap || etotal > cap;
    let mut outcome = if timed_out {
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

    // Export only from a workload that succeeded, and only once the container has
    // stopped. A failed export is a failed command: copying nothing and reporting success
    // is how a missing artefact becomes an invisible one.
    if let Some(policy) = &b.export {
        if outcome == Outcome::Passed {
            if let Err(why) = export_validated(&container, policy) {
                outcome = Outcome::ExportRefused(why);
            }
        }
    }

    let _ = Command::new("docker")
        .args(["rm", "-f", &container])
        .output();

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

/// Copies out exactly what the policy permits, or refuses and changes nothing.
///
/// The manifest is written by the workload, so every line of it is treated as a claim to
/// be checked rather than a fact. Artefacts are staged and validated first; the
/// destination is touched only once all of them pass, so a refusal cannot leave a
/// half-replaced baseline behind.
fn export_validated(container: &str, policy: &ExportPolicy) -> Result<(), String> {
    /// A manifest is a handful of short lines. Anything larger is not one.
    const MANIFEST_MAX: u64 = 4096;

    // Quiescence: a running container is still writing, and copying from one is copying
    // from a moving target.
    let running = Command::new("docker")
        .args(["inspect", container, "--format", "{{.State.Running}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false);
    if running {
        return Err(format!("{container} is still running; refusing to export"));
    }

    let stage = std::env::temp_dir().join(format!("nazm-export-{container}"));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("staging directory: {e}"))?;
    let cleanup = |s: &std::path::Path| {
        let _ = std::fs::remove_dir_all(s);
    };

    let result = (|| -> Result<Vec<(String, std::path::PathBuf)>, String> {
        let manifest_local = stage.join("EXPORTS");
        // Bounded during the transfer, not after it: a manifest larger than a manifest
        // could ever be is refused while the rest of it is still inside the container.
        copy_out_bounded(container, &policy.manifest, &manifest_local, MANIFEST_MAX)
            .map_err(|e| format!("manifest {}: {e}", policy.manifest))?;
        let text = std::fs::read_to_string(&manifest_local)
            .map_err(|e| format!("manifest is not text: {e}"))?;

        let dir = std::path::Path::new(&policy.manifest)
            .parent()
            .map_or_else(|| "/".to_owned(), |p| p.display().to_string());

        let mut seen: Vec<String> = Vec::new();
        let mut staged: Vec<(String, std::path::PathBuf)> = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let mut parts = line.split_whitespace();
            let (Some(name), Some(size), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(format!("malformed manifest line: {line:?}"));
            };
            if !policy.allowed.iter().any(|a| a == name) {
                return Err(format!(
                    "{name} is not permitted here; this run may export {:?}",
                    policy.allowed
                ));
            }
            if seen.iter().any(|s| s == name) {
                return Err(format!("{name} is listed twice"));
            }
            seen.push(name.to_owned());
            let declared: u64 = size
                .parse()
                .map_err(|_| format!("{name}: unreadable size {size:?}"))?;
            if declared > policy.max_bytes {
                return Err(format!(
                    "{name}: declared {declared} bytes, over this policy's {} budget",
                    policy.max_bytes
                ));
            }

            let local = stage.join(name);
            // The manifest's own claim is the transfer budget. Nothing crosses onto host
            // storage beyond the size it declared, and a symlink, directory or device node
            // is refused from the archive header — before any of it is extracted.
            let written = copy_out_bounded(container, &format!("{dir}/{name}"), &local, declared)
                .map_err(|e| format!("{name}: {e}"))?;
            if written != declared {
                return Err(format!(
                    "{name}: transferred {written} bytes, manifest declared {declared}"
                ));
            }
            staged.push((name.to_owned(), local));
        }
        if staged.is_empty() {
            return Err("the manifest named nothing".to_owned());
        }
        Ok(staged)
    })();

    let staged = match result {
        Ok(v) => v,
        Err(e) => {
            cleanup(&stage);
            return Err(e);
        }
    };

    // Everything validated; only now is the destination touched.
    for (name, local) in &staged {
        let dest = policy.dest.join(name);
        if let Err(e) = std::fs::copy(local, &dest) {
            cleanup(&stage);
            return Err(format!("{name}: could not place it: {e}"));
        }
        let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        eprintln!("contained: exported {name} ({bytes} bytes)");
    }
    cleanup(&stage);
    Ok(())
}

/// Copy one path out of a stopped container, refusing anything that is not a regular file
/// and stopping the moment the transfer exceeds `max` bytes.
///
/// `docker cp SRC DEST` writes the whole file to host storage and leaves every question
/// about it until afterwards, so a workload that emits a huge artefact has already filled
/// the disk by the time the check runs. Post-copy validation cannot enforce a transfer
/// budget — only the transfer can.
///
/// `docker cp SRC -` instead writes a tar stream to stdout, and a tar header states the
/// entry's type and size *before* any of its bytes follow. Reading the stream here means
/// a directory, a symlink, a device node or an oversized file is rejected while its
/// content is still inside the container, and that no more than `max` bytes are ever
/// written out. Returns how many bytes were written.
fn copy_out_bounded(
    container: &str,
    inside: &str,
    local: &std::path::Path,
    max: u64,
) -> Result<u64, String> {
    use std::io::{Read as _, Write as _};

    let mut child = Command::new("docker")
        .arg("cp")
        .arg(format!("{container}:{inside}"))
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run docker cp: {e}"))?;
    let Some(mut stream) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("docker cp gave no stream".to_owned());
    };

    // Every failure below goes through here: kill the transfer first, so a refused copy
    // cannot keep pulling bytes, then let docker's own message explain it if it has one.
    let stop = |child: &mut std::process::Child, why: String| -> String {
        let _ = child.kill();
        let said = child.stderr.take().map_or(String::new(), |s| {
            let mut buf = Vec::new();
            let _ = s.take(4096).read_to_end(&mut buf);
            String::from_utf8_lossy(&buf)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        });
        let _ = child.wait();
        if said.is_empty() {
            why
        } else {
            format!("{why} ({said})")
        }
    };

    let mut header = [0_u8; 512];
    if let Err(e) = stream.read_exact(&mut header) {
        return Err(stop(
            &mut child,
            format!("no tar header from docker cp: {e}"),
        ));
    }
    let at = |lo: usize, hi: usize| -> &[u8] { header.get(lo..hi).unwrap_or_default() };
    if at(257, 262) != b"ustar" {
        return Err(stop(&mut child, "not a tar stream".to_owned()));
    }

    // Type before size: reject what should never be extracted at all.
    match header.get(156).copied().unwrap_or(0) {
        b'0' | 0 => {}
        b'2' => return Err(stop(&mut child, format!("{inside} is a symlink"))),
        b'5' => return Err(stop(&mut child, format!("{inside} is a directory"))),
        // Measured on this daemon: `docker cp` writes plain ustar for the short names and
        // small sizes exported here, so a PAX header means something about the artefact
        // changed — a very long name, a size past the octal field. Named rather than
        // folded into the catch-all so it reads as "unhandled", not as "malicious".
        b'x' | b'g' => {
            return Err(stop(
                &mut child,
                format!("{inside}: the archive is PAX-extended, which this path does not read"),
            ))
        }
        t => {
            let kind = char::from(t);
            return Err(stop(
                &mut child,
                format!("{inside} is not a regular file (tar entry type {kind:?})"),
            ));
        }
    }

    // A size with the high bit set is base-256 encoded, which tar uses only for sizes too
    // large for the octal field — over 8 GiB, and so far over any budget here that the
    // encoding alone settles it.
    if header.get(124).copied().unwrap_or(0) & 0x80 != 0 {
        return Err(stop(
            &mut child,
            format!(
                "{inside}: size is base-256 encoded, vastly over the {max}-byte transfer budget"
            ),
        ));
    }
    let raw = String::from_utf8_lossy(at(124, 136)).into_owned();
    let digits = raw.trim_matches(|c: char| c == '\0' || c == ' ');
    let size = match u64::from_str_radix(digits, 8) {
        Ok(n) => n,
        Err(_) => {
            return Err(stop(
                &mut child,
                format!("{inside}: unreadable tar size field {digits:?}"),
            ))
        }
    };
    if size > max {
        return Err(stop(
            &mut child,
            format!(
                "{inside}: the archive header declares {size} bytes, over the {max}-byte transfer budget"
            ),
        ));
    }

    // Only now does anything reach host storage, and only `size` bytes of it.
    let mut file = match std::fs::File::create(local) {
        Ok(f) => f,
        Err(e) => return Err(stop(&mut child, format!("{inside}: {e}"))),
    };
    let mut buf = vec![0_u8; 64 * 1024];
    let mut left = size;
    while left > 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(0);
        let Some(slot) = buf.get_mut(..want) else {
            return Err(stop(&mut child, format!("{inside}: internal buffer error")));
        };
        let n = match stream.read(slot) {
            Ok(0) => {
                return Err(stop(
                    &mut child,
                    format!("{inside}: the stream ended {left} bytes early"),
                ))
            }
            Ok(n) => n,
            Err(e) => return Err(stop(&mut child, format!("{inside}: {e}"))),
        };
        let Some(got) = slot.get(..n) else {
            return Err(stop(&mut child, format!("{inside}: short read")));
        };
        if let Err(e) = file.write_all(got) {
            return Err(stop(&mut child, format!("{inside}: {e}")));
        }
        left -= n as u64;
    }
    if let Err(e) = file.flush() {
        return Err(stop(&mut child, format!("{inside}: {e}")));
    }
    drop(file);

    // The payload is in hand; the rest of the archive is padding and end-of-file blocks
    // that nothing here needs, so the transfer stops rather than draining it.
    let _ = child.kill();
    let _ = child.wait();
    Ok(size)
}

/// The platform slug of the image the workloads run in, measured by the supervisor.
///
/// Not taken from this machine: a Darwin host driving a Linux image would name
/// `baseline-darwin-arm64.json` for a file the container called
/// `baseline-linux-aarch64.json`, and the export would silently miss. Not taken from the
/// workload either, since that is the thing being checked. The shape matches
/// `bench::baseline_path`, which is what actually writes the file.
pub(crate) fn image_platform_slug() -> Result<String, String> {
    let out = Command::new("docker")
        .args(["run", "--rm", "--network", "none", IMAGE, "uname", "-sm"])
        .output()
        .map_err(|e| format!("could not run docker: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    let host = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if host.is_empty() {
        return Err("`uname -sm` in the image said nothing".to_owned());
    }
    Ok(host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    // The same allowance the integration tests carry: a fixture that cannot be built is a
    // broken test, and panicking says so at the line it happened.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use std::path::PathBuf;

    /// Small fixture image: anything with a shell. The export path never runs a workload,
    /// it only reads a stopped container, so the image's contents are irrelevant.
    const FIXTURE_IMAGE: &str = "alpine:3.20";

    fn docker_or_skip() -> bool {
        if std::env::var("NAZM_REQUIRE_DOCKER").as_deref() != Ok("1") {
            eprintln!("skipping: set NAZM_REQUIRE_DOCKER=1 to exercise the export path");
            return false;
        }
        let up = Command::new("docker")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(
            up,
            "NAZM_REQUIRE_DOCKER=1 but no daemon. These are the only tests that establish \
             the export path refuses what it should; skipping them silently would report \
             green on no evidence."
        );
        true
    }

    /// Leaves a *stopped* container whose `/out` holds whatever `script` created.
    fn fixture(name: &str, script: &str) -> String {
        let container = format!("nazm-exporttest-{name}-{}", std::process::id());
        let _ = Command::new("docker")
            .args(["rm", "-f", &container])
            .output();
        let out = Command::new("docker")
            .args([
                "run",
                "--name",
                &container,
                "--network",
                "none",
                FIXTURE_IMAGE,
            ])
            .args(["sh", "-c", &format!("mkdir -p /out && {script}")])
            .output()
            .expect("docker run");
        assert!(
            out.status.success(),
            "fixture {name} did not build: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        container
    }

    fn drop_fixture(c: &str) {
        let _ = Command::new("docker").args(["rm", "-f", c]).output();
    }

    fn policy(dest: &std::path::Path, allowed: &[&str]) -> ExportPolicy {
        ExportPolicy {
            manifest: "/out/EXPORTS".to_owned(),
            dest: dest.to_path_buf(),
            allowed: allowed.iter().map(|s| (*s).to_owned()).collect(),
            // The fixtures below declare five bytes and ship two megabytes, so the budget
            // has to sit between the two for the oversized case to be the thing under
            // test rather than the budget itself.
            max_bytes: 1024 * 1024,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nazm-exportdest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dest");
        d
    }

    #[test]
    fn a_named_artefact_of_the_declared_size_is_exported() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "ok",
            "printf 'hello' > /out/record.json && echo \"record.json 5\" > /out/EXPORTS",
        );
        let dest = scratch("ok");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(
            std::fs::read_to_string(dest.join("record.json")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn a_manifest_naming_a_file_that_is_not_there_is_refused() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture("missing", "echo \"record.json 5\" > /out/EXPORTS");
        let dest = scratch("missing");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "a missing artefact must refuse, got {r:?}");
        assert!(!dest.join("record.json").exists());
    }

    #[test]
    fn a_size_that_does_not_match_the_file_is_refused() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "mismatch",
            "printf 'hello' > /out/record.json && echo \"record.json 99\" > /out/EXPORTS",
        );
        let dest = scratch("mismatch");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "{r:?}");
        assert!(!dest.join("record.json").exists());
    }

    #[test]
    fn a_name_the_policy_does_not_permit_is_refused() {
        if !docker_or_skip() {
            return;
        }
        // A *check* run: the baseline is not among the permitted names.
        let c = fixture(
            "forbidden",
            "printf 'x' > /out/baseline.json && echo \"baseline.json 1\" > /out/EXPORTS",
        );
        let dest = scratch("forbidden");
        std::fs::write(dest.join("baseline.json"), "ORIGINAL").unwrap();
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "{r:?}");
        assert_eq!(
            std::fs::read_to_string(dest.join("baseline.json")).unwrap(),
            "ORIGINAL",
            "a refused export must leave the existing baseline exactly as it was"
        );
    }

    #[test]
    fn a_symlink_is_refused() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "symlink",
            "printf 'hello' > /out/real && ln -s real /out/record.json && echo \"record.json 5\" > /out/EXPORTS",
        );
        let dest = scratch("symlink");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "a symlink must refuse, got {r:?}");
    }

    #[test]
    fn an_oversized_manifest_is_refused_before_it_is_parsed() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "bigmanifest",
            "printf 'hello' > /out/record.json && i=0; while [ $i -lt 500 ]; do echo \"record.json 5\" >> /out/EXPORTS; i=$((i+1)); done",
        );
        let dest = scratch("bigmanifest");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "{r:?}");
        assert!(!dest.join("record.json").exists());
    }

    #[test]
    fn a_manifest_that_names_nothing_is_refused() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "empty",
            "printf 'hello' > /out/record.json && : > /out/EXPORTS",
        );
        let dest = scratch("empty");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "an empty manifest must refuse, got {r:?}");
    }

    #[test]
    fn a_malformed_line_is_refused() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "malformed",
            "printf 'hello' > /out/record.json && echo \"record.json 5 extra\" > /out/EXPORTS",
        );
        let dest = scratch("malformed");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        assert!(r.is_err(), "{r:?}");
    }

    /// The case post-copy validation cannot catch: a manifest declaring five bytes for an
    /// artefact holding two megabytes. The old path copied all two megabytes onto the host
    /// and only then compared sizes, so the budget was enforced after the damage.
    #[test]
    fn an_artefact_larger_than_it_declared_never_reaches_the_destination() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "oversized",
            "dd if=/dev/zero of=/out/record.json bs=1024 count=2048 2>/dev/null \
             && echo \"record.json 5\" > /out/EXPORTS",
        );
        let dest = scratch("oversized");
        let r = export_validated(&c, &policy(&dest, &["record.json"]));
        drop_fixture(&c);
        let why = format!("{r:?}");
        assert!(r.is_err(), "an oversized artefact must refuse, got {why}");
        assert!(
            why.contains("transfer budget"),
            "it must refuse during the transfer, not after it: {why}"
        );
        assert!(!dest.join("record.json").exists());
    }

    /// The same bound, measured on the transfer itself rather than through the policy:
    /// once the header is over budget nothing is written, so no file is created at all.
    #[test]
    fn the_transfer_stops_at_the_budget_before_writing() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture(
            "budget",
            "dd if=/dev/zero of=/out/record.json bs=1024 count=2048 2>/dev/null",
        );
        let dir = scratch("budget");
        let local = dir.join("record.json");
        let r = copy_out_bounded(&c, "/out/record.json", &local, 5);
        drop_fixture(&c);
        assert!(
            r.is_err(),
            "2 MiB against a 5-byte budget must refuse: {r:?}"
        );
        assert!(
            !local.exists(),
            "nothing may be written once the header is over budget"
        );
    }

    /// A non-regular entry is refused from the archive header, before extraction.
    #[test]
    fn a_directory_is_refused_from_the_header() {
        if !docker_or_skip() {
            return;
        }
        let c = fixture("dir", "mkdir -p /out/record.json");
        let dir = scratch("dir");
        let local = dir.join("record.json");
        let r = copy_out_bounded(&c, "/out/record.json", &local, 1024);
        drop_fixture(&c);
        let why = format!("{r:?}");
        assert!(r.is_err(), "a directory must refuse, got {why}");
        assert!(why.contains("directory"), "{why}");
        assert!(!local.exists());
    }
}
