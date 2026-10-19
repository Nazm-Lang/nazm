//! `cargo xtask host-run`: the one thing besides the contained runner that may start a program
//! written in Nazm on the macOS host (`docs/internal/runbook.md`, *What may run on the host: one
//! bounded-heap stage*; `openspec/changes/r6-host-qualification`, design decision 3).
//!
//! The rule it enforces is a ceiling from *inside*: a program built with `--heap-limit` counts
//! every byte it asks for and ends with `N0406` past its limit (`docs/user/spec.md`, *A bounded
//! heap*). So the runner does not trust a declaration; it reads the executable:
//!
//! - the counted heap is linked: the symbol `nz.heap_malloc` is in the file;
//! - the limit is the number in the program's own `N0406` message, which the counted heap embeds
//!   and prints (`crates/nazm-runtime/src/heap.rs`, `message`); it must be present, single and at
//!   most 2 GiB.
//!
//! Then it admits one stage at a time (a kernel lock on a file, plus a look for a contained
//! workload's container), runs it in the foreground in its own process group under a deadline
//! with its output drained and capped, and records its peak resident memory
//! (`/usr/bin/time -l`; no time is taken from it). A peak more than 256 MiB past the limit, or an
//! `N0406` that did not end the program with status 2, is the rule's **withdrawal condition**: it
//! is reported by that name, and a marker file makes every later run refuse until a person has
//! investigated and removed it.
//!
//! It is not a sandbox. It does not bound a task's stack, the image, C's allocations or children,
//! which is why it is one at a time and under a deadline.

use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use crate::contained;

/// The most heap a host stage may be built with: 2 GiB.
pub(crate) const MAX_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// How far past its limit a stage's peak resident memory may be before the rule is withdrawn.
pub(crate) const MARGIN: u64 = 256 * 1024 * 1024;
/// The deadline when none is given, and the longest one allowed.
const DEFAULT_DEADLINE: u64 = 900;
const MAX_DEADLINE: u64 = 3600;
/// The most of a program's stdout and of its stderr that is kept, unless `--max-output-mib` says
/// otherwise (never more than `MAX_OUTPUT_MIB`). Output past it is dropped and the run fails with
/// `EXIT_TRUNCATED`: a script comparing the output must never compare a prefix.
const OUTPUT_CAP: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_MIB: usize = 256;
/// The largest executable the runner will read to check.
const MAX_IMAGE: u64 = 512 * 1024 * 1024;

const SYMBOL: &[u8] = b"nz.heap_malloc";
const LIMIT_BEFORE: &[u8] = b"the heap limit of ";
const LIMIT_AFTER: &[u8] = b" bytes is reached";
const N0406: &str = "error[N0406]";

/// Why a run is refused, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotMacos,
    HostEnvironment(String),
    Usage(String),
    NoExecutable(String),
    NoCountedHeap,
    NoLimit,
    AmbiguousLimit(Vec<u64>),
    LimitTooLarge(u64),
    Withdrawn(String),
    AnotherStage,
    ContainedWorkload,
}

impl Refusal {
    /// The refusal's name, which the tests and the runbook cite.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::NotMacos => "not-macos",
            Self::HostEnvironment(_) => "host-environment",
            Self::Usage(_) => "usage",
            Self::NoExecutable(_) => "no-executable",
            Self::NoCountedHeap => "no-counted-heap",
            Self::NoLimit => "no-heap-limit",
            Self::AmbiguousLimit(_) => "ambiguous-heap-limit",
            Self::LimitTooLarge(_) => "heap-limit-too-large",
            Self::Withdrawn(_) => "withdrawn",
            Self::AnotherStage => "another-stage-running",
            Self::ContainedWorkload => "contained-workload-running",
        }
    }

    fn why(&self) -> String {
        match self {
            Self::NotMacos => {
                "the host runner is for the macOS host; elsewhere use `cargo xtask contained`"
                    .into()
            }
            Self::HostEnvironment(m) | Self::Usage(m) | Self::NoExecutable(m) => m.clone(),
            Self::NoCountedHeap => {
                "the executable does not link the counted heap (`nz.heap_malloc` is not in it): \
                 build it with `--heap-limit`"
                    .into()
            }
            Self::NoLimit => {
                "the executable carries no heap limit (its `N0406` message is not in it)".into()
            }
            Self::AmbiguousLimit(l) => format!("the executable names several heap limits: {l:?}"),
            Self::LimitTooLarge(l) => format!(
                "its heap limit is {l} bytes, over the 2 GiB ({MAX_LIMIT}) a host stage may have"
            ),
            Self::Withdrawn(m) => format!(
                "the rule is withdrawn: {m}. Host runs stay refused until it is investigated; \
                 then remove the marker file named above"
            ),
            Self::AnotherStage => {
                "another host stage this runner started is running: one at a time".into()
            }
            Self::ContainedWorkload => {
                "a contained workload is running: no host stage beside it".into()
            }
        }
    }
}

/// The heap limit an executable's bytes show, once they show the counted heap linked.
pub(crate) fn heap_limit_of(image: &[u8]) -> Result<u64, Refusal> {
    if !contains(image, SYMBOL) {
        return Err(Refusal::NoCountedHeap);
    }
    let mut found: Vec<u64> = Vec::new();
    let mut from = 0;
    while let Some(at) = find(image.get(from..).unwrap_or_default(), LIMIT_BEFORE) {
        let start = from + at + LIMIT_BEFORE.len();
        let rest = image.get(start..).unwrap_or_default();
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        let after = rest.get(digits..).unwrap_or_default();
        if digits > 0 && after.starts_with(LIMIT_AFTER) {
            if let Some(n) = rest
                .get(..digits)
                .and_then(|d| std::str::from_utf8(d).ok())
                .and_then(|d| d.parse::<u64>().ok())
            {
                if !found.contains(&n) {
                    found.push(n);
                }
            }
        }
        from = start;
    }
    match found.as_slice() {
        [] => Err(Refusal::NoLimit),
        [one] if *one > MAX_LIMIT => Err(Refusal::LimitTooLarge(*one)),
        [one] => Ok(*one),
        _ => Err(Refusal::AmbiguousLimit(found)),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

/// Peak resident memory in bytes from BSD `time -l`'s report (macOS reports bytes).
pub(crate) fn parse_peak_rss(report: &str) -> Option<u64> {
    report.lines().find_map(|l| {
        let n = l.trim().strip_suffix("maximum resident set size")?;
        n.trim().parse::<u64>().ok()
    })
}

/// Instructions retired from BSD `time -l`'s report (Apple silicon and recent Intel report it).
pub(crate) fn parse_instructions(report: &str) -> Option<u64> {
    report.lines().find_map(|l| {
        let n = l.trim().strip_suffix("instructions retired")?;
        n.trim().parse::<u64>().ok()
    })
}

/// The withdrawal condition a finished run shows, if any.
pub(crate) fn withdrawal(
    limit: u64,
    margin: u64,
    peak: Option<u64>,
    stderr: &str,
    code: Option<i32>,
) -> Option<String> {
    let Some(peak) = peak else {
        return Some("peak resident memory could not be read, so it cannot be compared".into());
    };
    if peak > limit.saturating_add(margin) {
        return Some(format!(
            "peak resident memory {peak} bytes exceeds the heap limit {limit} by more than {margin}"
        ));
    }
    if stderr.contains(N0406) && code != Some(2) {
        return Some(format!(
            "an N0406 was written but the program did not stop with status 2 (it ended {code:?})"
        ));
    }
    None
}

/// Whether `docker ps -q` output lists a contained workload's container.
pub(crate) fn lists_container(ps_quiet: &str) -> bool {
    ps_quiet.lines().any(|l| !l.trim().is_empty())
}

fn contained_running() -> bool {
    Command::new("docker")
        .args([
            "ps",
            "-q",
            "--filter",
            &format!("label={}", contained::RUN_OWNER),
        ])
        .output()
        .is_ok_and(|o| o.status.success() && lists_container(&String::from_utf8_lossy(&o.stdout)))
}

/// Where the lock and the withdrawal marker live. `NAZM_HOST_RUN_DIR` moves them, for the tests.
fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NAZM_HOST_RUN_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    home.join(".cache").join("nazm-host-run")
}

struct Options {
    exe: PathBuf,
    args: Vec<String>,
    deadline: u64,
    margin: u64,
    /// Where the run's report line is appended; standard error when it is not given.
    report_to: Option<PathBuf>,
    output_cap: usize,
}

fn parse_args(args: &[String]) -> Result<Options, Refusal> {
    let usage = |m: &str| {
        Refusal::Usage(format!(
            "{m}\nusage: cargo xtask host-run [--deadline-secs N] [--report FILE] [--max-output-mib N] -- EXECUTABLE [ARGS...]"
        ))
    };
    let mut deadline = DEFAULT_DEADLINE;
    let mut margin = MARGIN;
    let mut report_to = None;
    let mut output_cap = OUTPUT_CAP;
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match a.as_str() {
            "--" => {
                i += 1;
                break;
            }
            "--deadline-secs" => {
                deadline = args
                    .get(i + 1)
                    .and_then(|n| n.parse().ok())
                    .filter(|n| (1..=MAX_DEADLINE).contains(n))
                    .ok_or_else(|| usage(&format!("--deadline-secs takes 1..={MAX_DEADLINE}")))?;
                i += 2;
            }
            "--max-output-mib" => {
                output_cap = args
                    .get(i + 1)
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|n| (1..=MAX_OUTPUT_MIB).contains(n))
                    .ok_or_else(|| {
                        usage(&format!("--max-output-mib takes 1..={MAX_OUTPUT_MIB}"))
                    })?
                    * 1024
                    * 1024;
                i += 2;
            }
            "--report" => {
                report_to = Some(PathBuf::from(
                    args.get(i + 1)
                        .ok_or_else(|| usage("--report takes a file"))?,
                ));
                i += 2;
            }
            // Only ever tighter than the rule's 256 MiB: for the tests of the breach report.
            "--margin-bytes" => {
                margin = args
                    .get(i + 1)
                    .and_then(|n| n.parse().ok())
                    .filter(|n| *n <= MARGIN)
                    .ok_or_else(|| usage("--margin-bytes takes at most 268435456"))?;
                i += 2;
            }
            other => return Err(usage(&format!("unknown option `{other}`"))),
        }
    }
    let Some(exe) = args.get(i) else {
        return Err(usage("no executable named"));
    };
    Ok(Options {
        exe: PathBuf::from(exe),
        args: args.get(i + 1..).unwrap_or_default().to_vec(),
        deadline,
        margin,
        report_to,
        output_cap,
    })
}

fn read_image(exe: &Path) -> Result<Vec<u8>, Refusal> {
    let bad = |m: String| Refusal::NoExecutable(format!("{}: {m}", exe.display()));
    let meta = fs::metadata(exe).map_err(|e| bad(e.to_string()))?;
    if !meta.is_file() {
        return Err(bad("not a regular file".into()));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 == 0 {
        return Err(bad("not executable".into()));
    }
    if meta.len() > MAX_IMAGE {
        return Err(bad(format!("larger than {MAX_IMAGE} bytes")));
    }
    let mut bytes = Vec::new();
    fs::File::open(exe)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| bad(e.to_string()))?;
    Ok(bytes)
}

/// `cargo xtask host-run [--deadline-secs N] -- EXECUTABLE [ARGS...]`.
pub(crate) fn host_run(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(code) => code,
        Err(r) => {
            eprintln!("host-run: refused [{}]: {}", r.name(), r.why());
            ExitCode::from(EXIT_REFUSED)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, Refusal> {
    if !cfg!(target_os = "macos") {
        return Err(Refusal::NotMacos);
    }
    if let Some(m) = contained::host_env_refusal(|v| std::env::var(v).ok()) {
        return Err(Refusal::HostEnvironment(m));
    }
    let opts = parse_args(args)?;
    let limit = heap_limit_of(&read_image(&opts.exe)?)?;

    let dir = state_dir();
    let state = |e: std::io::Error| Refusal::Usage(format!("{}: {e}", dir.display()));
    fs::create_dir_all(&dir).map_err(state)?;
    let marker = dir.join("withdrawn");
    if let Ok(why) = fs::read_to_string(&marker) {
        return Err(Refusal::Withdrawn(format!(
            "{} (marker: {})",
            why.trim(),
            marker.display()
        )));
    }
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("lock"))
        .map_err(state)?;
    if lock.try_lock().is_err() {
        return Err(Refusal::AnotherStage);
    }
    if contained_running() {
        return Err(Refusal::ContainedWorkload);
    }

    let report = dir.join(format!("time-{}.txt", std::process::id()));
    let result = execute(&opts, &report);
    let _ = fs::remove_file(&report);
    let ran = result.map_err(Refusal::Usage)?;

    // The program's standard output is the only thing on this process's standard output, so a
    // script can compare it; the report goes with the program's standard error, after it.
    {
        use std::io::Write as _;
        let _ = std::io::stdout().write_all(&ran.stdout);
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().write_all(&ran.stderr);
    }
    let stderr_text = String::from_utf8_lossy(&ran.stderr).into_owned();
    let line = format!(
        "host-run: heap limit {limit} bytes; peak resident {} bytes; {}",
        ran.peak.map_or_else(|| "unknown".into(), |p| p.to_string()),
        if ran.timed_out {
            "DEADLINE reached, the stage was killed".to_owned()
        } else {
            format!("exit {:?}", ran.code)
        }
    ) + &ran
        .instructions
        .map_or_else(String::new, |n| format!("; instructions retired {n}"))
        + &if ran.truncated {
            format!(
                "; OUTPUT TRUNCATED at {} bytes (stdout {} bytes, stderr {} bytes)",
                opts.output_cap, ran.stdout_total, ran.stderr_total
            )
        } else {
            String::new()
        };
    // With `--report FILE` the report is appended there and standard error is the program's alone,
    // so a script can compare it.
    match &opts.report_to {
        None => eprintln!("{line}"),
        Some(file) => {
            use std::io::Write as _;
            let written = fs::File::options()
                .create(true)
                .append(true)
                .open(file)
                .and_then(|mut f| writeln!(f, "{line}"));
            if let Err(e) = written {
                return Err(Refusal::Usage(format!("{}: {e}", file.display())));
            }
        }
    }
    // A stage killed at its deadline leaves no resource report to read: not a breach.
    let unreadable_after_kill = ran.timed_out && ran.peak.is_none();
    if let Some(why) = withdrawal(limit, opts.margin, ran.peak, &stderr_text, ran.code)
        .filter(|_| !unreadable_after_kill)
    {
        let _ = fs::write(&marker, format!("{why} (running {})", opts.exe.display()));
        eprintln!(
            "host-run: WITHDRAWAL CONDITION: {why}. The host rule is withdrawn: only \
             `cargo xtask contained` until it is investigated (marker: {})",
            marker.display()
        );
        return Ok(ExitCode::from(EXIT_WITHDRAWN));
    }
    if ran.truncated && !ran.timed_out {
        return Ok(ExitCode::from(EXIT_TRUNCATED));
    }
    Ok(ExitCode::from(exit_status(ran.code, ran.timed_out)))
}

/// Status of a refused run, a stage past its deadline, a stage killed by a signal and a withdrawal.
/// They are the shell's own conventions where it has one (`timeout` uses 124), so a program's
/// status never has to be guessed from them: a script that needs the program's own status compares
/// it with these.
pub(crate) const EXIT_REFUSED: u8 = 1;
pub(crate) const EXIT_DEADLINE: u8 = 124;
pub(crate) const EXIT_SIGNALLED: u8 = 125;
pub(crate) const EXIT_WITHDRAWN: u8 = 70;
/// Output past `--max-output-mib` was dropped: what a script compared would be a prefix.
pub(crate) const EXIT_TRUNCATED: u8 = 126;

/// The status `host-run` exits with: the program's own, passed through.
pub(crate) fn exit_status(code: Option<i32>, timed_out: bool) -> u8 {
    if timed_out {
        return EXIT_DEADLINE;
    }
    match code {
        Some(c) => u8::try_from(c & 0xff).unwrap_or(EXIT_SIGNALLED),
        None => EXIT_SIGNALLED,
    }
}

struct Ran {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_total: u64,
    stderr_total: u64,
    truncated: bool,
    code: Option<i32>,
    peak: Option<u64>,
    instructions: Option<u64>,
    timed_out: bool,
}

fn execute(opts: &Options, report: &Path) -> Result<Ran, String> {
    let mut cmd = Command::new("/usr/bin/time");
    cmd.arg("-l")
        .arg("-o")
        .arg(report)
        .arg(&opts.exe)
        .args(&opts.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start /usr/bin/time: {e}"))?;
    let out = child
        .stdout
        .take()
        .map(|s| contained::drain(s, opts.output_cap));
    let err = child
        .stderr
        .take()
        .map(|s| contained::drain(s, opts.output_cap));
    let pgid = child.id();
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => return Err(format!("waiting for the stage: {e}")),
        }
        if start.elapsed() >= Duration::from_secs(opts.deadline) {
            timed_out = true;
            // Our own process group, which we created for this one stage.
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pgid}")])
                .status();
            let _ = child.kill();
            break child
                .wait()
                .map_err(|e| format!("reaping the stage: {e}"))?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let take = |rx: Option<std::sync::mpsc::Receiver<(Vec<u8>, u64)>>| {
        rx.and_then(|r| r.recv_timeout(Duration::from_secs(5)).ok())
            .unwrap_or_default()
    };
    let (stdout, stdout_total) = take(out);
    let (stderr, stderr_total) = take(err);
    let time_report = fs::read_to_string(report).ok();
    let peak = time_report.as_deref().and_then(parse_peak_rss);
    let instructions = time_report.as_deref().and_then(parse_instructions);
    Ok(Ran {
        truncated: stdout_total > stdout.len() as u64 || stderr_total > stderr.len() as u64,
        stdout,
        stderr,
        stdout_total,
        stderr_total,
        code: status.code(),
        peak,
        instructions,
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(limit: &str, symbol: bool) -> Vec<u8> {
        let mut v = b"\0junk\0".to_vec();
        if symbol {
            v.extend_from_slice(b"_nz.heap_malloc\0");
        }
        v.extend_from_slice(
            format!(
                "nazm: error[N0406]: out of memory: the heap limit of {limit} bytes is reached\n"
            )
            .as_bytes(),
        );
        v
    }

    #[test]
    fn a_limit_is_read_from_the_programs_own_message() {
        assert_eq!(heap_limit_of(&image("4194304", true)), Ok(4_194_304));
        assert_eq!(heap_limit_of(&image("2147483648", true)), Ok(MAX_LIMIT));
    }

    #[test]
    fn a_program_without_the_counted_heap_is_refused() {
        assert_eq!(
            heap_limit_of(&image("4194304", false)),
            Err(Refusal::NoCountedHeap)
        );
    }

    #[test]
    fn a_program_that_names_no_limit_is_refused() {
        assert_eq!(
            heap_limit_of(b"nz.heap_malloc and nothing else"),
            Err(Refusal::NoLimit)
        );
    }

    #[test]
    fn a_limit_over_two_gib_is_refused() {
        assert_eq!(
            heap_limit_of(&image("2147483649", true)),
            Err(Refusal::LimitTooLarge(2_147_483_649))
        );
    }

    #[test]
    fn two_different_limits_are_refused() {
        let mut v = image("1048576", true);
        v.extend(image("2097152", true));
        assert_eq!(
            heap_limit_of(&v),
            Err(Refusal::AmbiguousLimit(vec![1_048_576, 2_097_152]))
        );
    }

    #[test]
    fn the_instruction_count_is_read_from_the_time_report() {
        let r = "  1.00 real\n   123456 instructions retired\n  99 maximum resident set size\n";
        assert_eq!(parse_instructions(r), Some(123_456));
        assert_eq!(parse_instructions("no counter here\n"), None);
    }

    #[test]
    fn the_peak_is_read_from_the_time_report() {
        let t = "        0.00 real         0.00 user         0.00 sys\n   1802240  maximum resident set size\n";
        assert_eq!(parse_peak_rss(t), Some(1_802_240));
        assert_eq!(parse_peak_rss("nothing"), None);
    }

    #[test]
    fn a_peak_past_the_limit_and_margin_withdraws_the_rule() {
        let l = 1 << 20;
        assert!(withdrawal(l, MARGIN, Some(l + MARGIN), "", Some(0)).is_none());
        assert!(withdrawal(l, MARGIN, Some(l + MARGIN + 1), "", Some(0)).is_some());
        assert!(withdrawal(l, MARGIN, None, "", Some(0)).is_some());
    }

    #[test]
    fn an_n0406_that_did_not_stop_the_program_withdraws_the_rule() {
        let l = 1 << 20;
        let said = "nazm: error[N0406]: out of memory";
        assert!(withdrawal(l, MARGIN, Some(1), said, Some(2)).is_none());
        assert!(withdrawal(l, MARGIN, Some(1), said, Some(0)).is_some());
        assert!(withdrawal(l, MARGIN, Some(1), said, None).is_some());
    }

    #[test]
    fn the_programs_exit_status_is_passed_through() {
        assert_eq!(exit_status(Some(0), false), 0);
        assert_eq!(exit_status(Some(2), false), 2);
        assert_eq!(exit_status(Some(255), false), 255);
        assert_eq!(exit_status(None, false), EXIT_SIGNALLED);
        assert_eq!(exit_status(Some(0), true), EXIT_DEADLINE);
        assert_eq!(exit_status(Some(3), true), EXIT_DEADLINE);
    }

    #[test]
    fn a_contained_workload_is_seen_in_docker_ps_output() {
        assert!(lists_container("0123abcd\n"));
        assert!(!lists_container("\n  \n"));
        assert!(!lists_container(""));
    }

    #[test]
    fn the_margin_may_only_be_tightened() {
        let a = |v: &[&str]| parse_args(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert!(a(&["--margin-bytes", "0", "--", "x"]).is_ok());
        assert!(a(&["--margin-bytes", "268435457", "--", "x"]).is_err());
        assert!(a(&["--deadline-secs", "0", "--", "x"]).is_err());
        assert!(a(&["--deadline-secs", "3601", "--", "x"]).is_err());
        assert!(a(&["--"]).is_err());
    }
}
