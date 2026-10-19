//! The gate every generated compiler stage passes through before it is allowed to run.
//!
//! # Why this exists
//!
//! On 2026-09-21 `cargo test --test selfhost` stopped the machine three times. The suite
//! builds `nzlex`, `nzparse`, `nzcheck` and `nzemit` and runs them natively with no
//! ceiling; the drivers accumulated their output by copying, so each grew without bound,
//! and `cargo test` runs one test thread per core — ten of them at once on a 32 GiB
//! machine. The kernel's userspace watchdog panicked because nothing could be scheduled.
//!
//! The accumulation bug is fixed (see `tests/resource_regression.rs`). This gate is the
//! other half: a bug of that shape must not be able to reach the host again. macOS cannot
//! impose a hard memory ceiling on a child — `RLIMIT_AS` is an alias of `RLIMIT_RSS`
//! there and `setrlimit(2)` documents it as advisory — so the ceiling has to come from
//! outside the process, and the refusal has to be explicit rather than a silent skip.

// Every integration test that needs `bounded_exe` includes this whole file, and only the
// ones that *build a stage executable* call `require_containment`. So in the others it is
// genuinely unused, and `-D warnings` makes that fatal. Allowed here rather than by
// splitting the module, because the two halves belong together: the gate and the ceiling
// are one subject, and a reader looking for either should find both.
#![allow(dead_code)]

use std::sync::Once;

/// Set inside the bounded environment, where a real ceiling already applies.
const CONTAINED: &str = "NAZM_CONTAINED";
/// Deliberate, documented waiver. Loud on purpose.
const WAIVER: &str = "NAZM_ALLOW_UNCONTAINED";

/// Refuses to let a generated compiler stage run without a resource ceiling.
///
/// Call this from every function that produces a stage executable, so no launch site can
/// bypass it by construction.
pub(crate) fn require_containment(stage: &str) {
    if std::env::var(CONTAINED).as_deref() == Ok("1") {
        // A declaration, checked: the contained runner's cgroup caps memory, so `memory.max`
        // is a number there. On a host — macOS has no cgroup at all — it is not, and the
        // variable is the forbidden host configuration rather than a ceiling (Q1-A2-02).
        let max = std::fs::read_to_string("/sys/fs/cgroup/memory.max").unwrap_or_default();
        assert!(
            max.trim().parse::<u64>().is_ok_and(|n| n > 0),
            "{CONTAINED}=1 is set, but no memory ceiling applies here (`/sys/fs/cgroup/memory.max` \
             is {:?}). Run `{stage}` through `cargo xtask contained`; never set {CONTAINED} on a host.",
            max.trim()
        );
        return;
    }
    if std::env::var(WAIVER).as_deref() == Ok("1") {
        static WARNED: Once = Once::new();
        WARNED.call_once(|| {
            eprintln!(
                "warning: {WAIVER}=1 — generated compiler stages are running with no memory \
                 ceiling. This is the configuration that panicked the machine on 2026-09-21."
            );
        });
        return;
    }
    panic!(
        "refusing to build and run `{stage}` with no resource ceiling.\n\
         \n\
         This suite executes compiler stages written in Nazm. A stage that allocates \
         without bound has no ceiling on this host: macOS `RLIMIT_AS` is advisory, so a \
         runaway reaches the machine rather than the process. On 2026-09-21 that panicked \
         the kernel three times.\n\
         \n\
         Run it inside the bounded environment, one workload at a time:\n\
         \n    NAZM_CONTAINED=1 cargo test -p nazm-cli --test selfhost --jobs 1 -- --test-threads 1\n\
         \n\
         and set {CONTAINED}=1 only where a real ceiling applies (a container with \
         --memory and --memory-swap set equal, or a cgroup that enforces one).\n\
         To override deliberately and accept the risk, set {WAIVER}=1."
    );
}

/// The address-space ceiling a generated program runs under, in kilobytes: 512 MiB.
///
/// Chosen against measurement, not taste. The largest legitimate program in the tree
/// peaks at 4.4 MB resident, the self-hosted compiler stages at about 36 MB, and each
/// pthread a concurrency test spawns reserves roughly 8 MB of address space for its
/// stack. 512 MiB clears all of that by two orders of magnitude while sitting well below
/// the container's own 4 GiB ceiling — so a runaway dies as a *test failure*, attributed
/// to the test that caused it, instead of taking the whole run down with an
/// infrastructure OOM.
const ADDRESS_SPACE_KB: u64 = 512 * 1024;

/// The ceiling for a program whose subject is many live tasks, in kilobytes: 4 GiB, the
/// container's own.
///
/// v1 starts one OS thread per task, and on Linux each thread costs address space well past
/// its stack: glibc gives a thread its own malloc arena and reserves 64 MiB for it, so a few
/// dozen live tasks pass 512 MiB while touching a few megabytes. Measured in the contained gate
/// (N48): the five-thousand-task test was refused a thread at about forty live tasks
/// under [`ADDRESS_SPACE_KB`]. A runaway still dies at the container's ceiling.
pub(crate) const TASKS_ADDRESS_SPACE_KB: u64 = 4 * 1024 * 1024;

/// A `Command` that runs `exe` under that ceiling.
///
/// # What this bounds, and what it does not
///
/// `ulimit -v` sets `RLIMIT_AS`, which is **per-process virtual address space**. Two
/// things follow, and both matter:
///
/// - It is *virtual*, not resident. A process that maps a great deal and touches little
///   is refused even though its footprint is small, which is why the value is generous
///   rather than tight. It is still the right instrument here, because the failure being
///   guarded against is unbounded `malloc`, and that grows the address space.
/// - It is *per process*, **not an aggregate over descendants**. Each child inherits the
///   same limit and gets its own allowance, so `n` children can hold `n` times this
///   figure between them. This bounds one runaway; it does not bound a fleet. The
///   container's `--memory` remains the only aggregate ceiling, and nothing here is a
///   substitute for it.
///
/// Threads are covered, since they share one address space with the process that spawned
/// them.
///
/// **Enforcement differs by platform, so the wrapper does too.** On Linux the limit is
/// real, and failing to establish it is treated as a failure: the wrapper exits 97 rather
/// than running the program unbounded, because silently proceeding is how a guard becomes
/// decoration. On Darwin `RLIMIT_AS` is an alias of `RLIMIT_RSS` and `setrlimit(2)`
/// documents it as advisory, so the attempt is best-effort there and its failure is not
/// an error.
///
/// A shell wrapper rather than `pre_exec`, because the workspace sets
/// `unsafe_code = "forbid"` and `pre_exec` is unsafe.
pub(crate) fn bounded_exe(exe: &std::path::Path) -> std::process::Command {
    bounded_exe_within(exe, ADDRESS_SPACE_KB)
}

/// [`bounded_exe`] under a ceiling of `kb` kilobytes rather than [`ADDRESS_SPACE_KB`].
pub(crate) fn bounded_exe_within(exe: &std::path::Path, kb: u64) -> std::process::Command {
    // Windows has no `RLIMIT_AS`, and no `sh` of its own: the program runs directly, under the
    // run's deadline, which is the bound a test can still keep there.
    if cfg!(windows) {
        return std::process::Command::new(exe);
    }
    let script = format!(
        r#"if [ "$(uname -s)" = Linux ]; then
    ulimit -v {kb} || {{ printf 'bounded_exe: cannot set RLIMIT_AS
' >&2; exit 97; }}
    got=$(ulimit -v)
    if [ "$got" != {kb} ]; then
        printf 'bounded_exe: RLIMIT_AS reads %s, expected {kb}
' "$got" >&2
        exit 97
    fi
else
    ulimit -v {kb} 2>/dev/null || true
fi
exec "$0" "$@""#
    );
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(script).arg(exe);
    c
}

/// What one bounded run of a child produced.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// How long [`run_bounded`] lets a child run.
pub(crate) const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);
/// The most of one stream [`run_bounded`] keeps; the rest is read and dropped, so a child is
/// never blocked on a full pipe.
const KEEP: usize = 16 << 20;

/// Run `cmd` to completion within [`DEADLINE`], both streams drained as it runs.
///
/// M1: ten suites carried one copy of this that read the pipes only after the child exited,
/// so a child writing more than a pipe holds (about 64 KiB) blocked, and was reported as not
/// terminating. Draining on two threads removes that; output past [`KEEP`] bytes per stream is
/// discarded rather than buffered. On the deadline the child is killed and reaped and the test
/// fails, naming `what`.
pub(crate) fn run_bounded(cmd: &mut std::process::Command, what: &str) -> Ran {
    run_within(cmd, DEADLINE)
        .unwrap_or_else(|| panic!("{what} did not terminate within {DEADLINE:?}"))
}

/// [`run_bounded`]'s run with its own `limit`, `None` when the child outlived it (killed and
/// reaped): for a suite that reports "did not terminate" in its own words. Q1 found two suites,
/// `build.rs` and `concurrent.rs`, still carrying the copy M1 meant to remove — the one that reads
/// the pipes only after the child exits.
pub(crate) fn run_within(
    cmd: &mut std::process::Command,
    limit: std::time::Duration,
) -> Option<Ran> {
    run_within_stdin(cmd, limit, std::process::Stdio::null())
}

/// [`run_within`] with the child's standard input `stdin` (a file, where a test would have a shell
/// redirect one: Windows has no `sh`).
pub(crate) fn run_within_stdin(
    cmd: &mut std::process::Command,
    limit: std::time::Duration,
    stdin: std::process::Stdio,
) -> Option<Ran> {
    use std::io::Read;
    use std::process::Stdio;

    fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = [0_u8; 8192];
            while let Ok(n) = r.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let room = KEEP.saturating_sub(kept.len());
                kept.extend_from_slice(chunk.get(..n.min(room)).unwrap_or_default());
            }
            kept
        })
    }

    let mut child = cmd
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let out = drain(child.stdout.take().expect("stdout pipe"));
    let err = drain(child.stderr.take().expect("stderr pipe"));
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().expect("try_wait") {
            break s;
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            let _ = (out.join(), err.join());
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let text = |h: std::thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&h.join().unwrap_or_default()).into_owned()
    };
    Some(Ran {
        code: status.code(),
        stdout: text(out),
        stderr: text(err),
    })
}

/// Whether a test that needs a host tool must stop here, having found it absent (Q1-D-02).
///
/// A test that returns early passes, and a pass on no evidence is a defect (`docs/runbook.md`,
/// *Status vocabulary*). So a test bound to a platform is `#[ignore]`d elsewhere by `cfg_attr`,
/// and counted as ignored there; on its own platform a missing tool still skips — printing `SKIP`
/// — unless `NAZM_REQUIRE_HOST_TOOLS=1`, which the named host pass sets, turns the skip into a
/// failure.
pub(crate) fn skip_without(present: bool, what: &str) -> bool {
    if present {
        return false;
    }
    assert!(
        std::env::var("NAZM_REQUIRE_HOST_TOOLS").as_deref() != Ok("1"),
        "NAZM_REQUIRE_HOST_TOOLS=1, and {what}"
    );
    eprintln!("SKIP: {what}");
    true
}

/// The units a build has beyond the portable ones where the target does something before `main`:
/// Windows's `init` unit (binary standard streams), which every Windows build carries; nothing
/// elsewhere. A test that counts a build's units adds it.
pub(crate) const PLATFORM_UNITS: u64 = if cfg!(windows) { 1 } else { 0 };
/// [`PLATFORM_UNITS`] as a length.
pub(crate) const PLATFORM_UNITS_LEN: usize = if cfg!(windows) { 1 } else { 0 };

/// What a C program linking a Nazm static library needs beyond the library: on Windows the runtime's
/// threads, sockets and random bytes (the libraries `nazm build` links itself); nothing elsewhere.
pub(crate) fn c_host_libs() -> &'static [&'static str] {
    if cfg!(windows) {
        &["-pthread", "-lws2_32", "-lbcrypt"]
    } else {
        &[]
    }
}

/// The return type of a generated function that can fail, in LLVM text: on Windows, where a
/// failure is a returned flag rather than an unwind, the value paired with it.
pub(crate) fn ret(t: &str) -> String {
    if cfg!(windows) {
        format!("{{{t}, i1}}")
    } else {
        t.to_owned()
    }
}

/// The name `nazm build -o NAME` publishes on this platform: `NAME` plus the platform's
/// executable suffix (`.exe` on Windows, where the build adds it to a name with no extension;
/// nothing elsewhere). Every test that starts, reads or looks for a program it built names it
/// through this.
pub(crate) fn exe_name(name: impl AsRef<str>) -> String {
    format!("{}{}", name.as_ref(), std::env::consts::EXE_SUFFIX)
}
