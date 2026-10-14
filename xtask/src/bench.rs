//! `cargo xtask bench` — measurement, recorded so it can be argued with.
//!
//! # What makes this reproducible, and what does not
//!
//! Every run records the machine, the toolchain, the commit and whether the tree was
//! clean, because a number without those is not evidence of anything. What it **cannot**
//! record is the machine's thermal state, what else was running, or how the CPU chose to
//! scale its frequency — so the honest unit of comparison is two runs on one machine
//! minutes apart, and `--check` refuses to compare across hosts rather than producing a
//! percentage that means nothing.
//!
//! # Minimum and median, and why both
//!
//! The minimum is the least noisy estimate of how fast the work *can* go: noise only
//! ever adds time. The median is what someone actually waiting would see. A claim made
//! from the minimum alone hides a program that is usually slow, and one from the median
//! alone hides an improvement inside the noise, so both are recorded and the baseline
//! comparison uses the median.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

/// The version every `bench/*.json` carries. See `docs/performance.md`.
const SCHEMA: &str = "nazm.bench/2";

/// How far a median may move before `--check` calls it a change.
///
/// Wide, deliberately. On a laptop with no frequency pinning, run-to-run variation of
/// ten per cent is ordinary, and a gate that fires on ordinary variation is a gate
/// people disable. This is for catching a regression of the kind that matters — a
/// doubling — not for detecting a five per cent win.
const TOLERANCE: f64 = 0.25;

/// The smallest absolute move worth reporting, in milliseconds.
///
/// A ratio alone is the wrong test near zero, and the first `--check` run proved it: it
/// reported `arith/check` at 3.3 -> 5.8 ms as a regression on a code path that had not
/// been touched. Measuring the same command fifteen times in one burst gives a spread
/// of 1.13x to 1.34x on anything in the three-to-eight millisecond range, and across
/// bursts minutes apart it is wider still — process start-up and filesystem cache state
/// are most of what is being timed there.
///
/// So a change must clear **both** bars. The consequence is stated rather than hidden:
/// every measurement under about fifteen milliseconds is below this method's noise
/// floor and is recorded rather than gated. Measuring those properly needs a harness
/// that does not pay process start-up per sample, which this is not.
const FLOOR_MS: f64 = 5.0;

/// Where cargo actually put its artefacts.
///
/// `$root/target` is only right when nobody redirected it. The contained runner mounts
/// the checkout read-only and sets `CARGO_TARGET_DIR` elsewhere, both because the mount
/// cannot be written and because the checkout's own `target/` holds
/// aarch64-apple-darwin artefacts that must not be mixed with Linux ones. Resolving this
/// by hand rather than by convention is what lets the same command work in both places.
/// Where the record and the baseline are written.
///
/// `$root/bench` unless `NAZM_BENCH_DIR` says otherwise. The contained runner points it
/// at bounded container storage, because the alternative — mounting the checkout
/// read-write so the results can land in it — puts every write on the host filesystem,
/// where `--storage-opt` does not reach. A quota on the container's writable layer bounds
/// nothing that goes through a writable bind mount.
pub(crate) fn bench_dir(root: &Path) -> PathBuf {
    std::env::var_os("NAZM_BENCH_DIR").map_or_else(|| root.join("bench"), PathBuf::from)
}

pub(crate) fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| crate::guide::root().join("target"), PathBuf::from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Measure and write `bench/record.json`.
    Record,
    /// Measure, and also replace `bench/baseline.json`.
    Save,
    /// Measure, and compare against the baseline.
    Check,
}

/// A named way of invoking the compiler, and the command that does it.
///
/// A type alias because the tuple is unreadable inline, which clippy is right about:
/// `(&'static str, Box<dyn Fn() -> Command>)` at a `let` tells a reader nothing.
type Stage = (&'static str, Box<dyn Fn() -> Command>);

/// Peak resident memory for one command, in bytes.
///
/// `/usr/bin/time -l` rather than anything in-process: measuring a *child's* peak needs
/// `getrusage(RUSAGE_CHILDREN)`, which this workspace cannot reach without `unsafe`, and
/// the lint is `forbid` for better reasons than this.
///
/// **Platform-specific, and recorded as such.** On Darwin the field is in bytes; on a
/// platform where `time -l` means something else, the number is whatever that platform
/// reports, and the record says which host it was taken on. `None` when the line was not
/// found — missing is representable, and a peak of zero is not a measurement.
#[derive(Debug)]
struct Memory {
    program: String,
    stage: &'static str,
    peak_bytes: Option<u64>,
}

fn peak_rss(cmd: &mut Command) -> Option<u64> {
    // GNU `time -v` first: it reads `getrusage`, so it is exact even for a process that
    // exits immediately. Polling `/proc` cannot be — a short program can be gone before
    // the first read, and reporting the low value caught on the way past would be a
    // measurement that is quietly wrong. Missing is representable; wrong is not.
    if let Some(kb) = peak_rss_gnu_time(cmd) {
        return Some(kb * 1024);
    }
    if Path::new("/proc/self/status").exists() {
        return peak_rss_proc(cmd);
    }
    let mut wrapped = Command::new("/usr/bin/time");
    wrapped.arg("-l").arg(cmd.get_program());
    for a in cmd.get_args() {
        wrapped.arg(a);
    }
    let out = wrapped.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .find(|l| l.contains("maximum resident set size"))
        .and_then(|l| l.split_whitespace().next()?.parse().ok())
}

/// Peak RSS in kilobytes from GNU `time -v`, which reports `getrusage(RUSAGE_CHILDREN)`.
///
/// `None` when GNU time is absent or its output does not carry the line — BSD `time`
/// takes `-l` and would reject `-v`, and the slim Debian image ships neither until
/// `procps`/`time` are installed.
fn peak_rss_gnu_time(cmd: &mut Command) -> Option<u64> {
    let mut wrapped = Command::new("/usr/bin/time");
    wrapped.arg("-v").arg(cmd.get_program());
    for a in cmd.get_args() {
        wrapped.arg(a);
    }
    let out = wrapped.output().ok()?;
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find(|l| l.contains("Maximum resident set size"))
        .and_then(|l| l.rsplit(' ').next()?.parse().ok())
}

/// Peak RSS on Linux, from `/proc/<pid>/status`. Last resort.
///
/// `/usr/bin/time -l` is the BSD spelling and the binary is absent from a slim Debian
/// entirely, so every memory row read "not measured" the moment the benchmarks moved into
/// the contained runner. `VmHWM` is the kernel's own high-water mark, so polling cannot
/// under-report a peak that has already happened — it only has to be read before the
/// process is reaped. Reported in bytes, as the Darwin path is.
fn peak_rss_proc(cmd: &mut Command) -> Option<u64> {
    let mut child = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let status = format!("/proc/{}/status", child.id());
    let mut hwm_kb = 0u64;
    loop {
        if let Ok(text) = std::fs::read_to_string(&status) {
            if let Some(kb) = text
                .lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            {
                hwm_kb = hwm_kb.max(kb);
            }
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(_) => break,
        }
    }
    // A real process does not peak below a megabyte; anything smaller means the poll
    // lost the race with a short program rather than measured it.
    (hwm_kb >= 1024).then(|| hwm_kb * 1024)
}

/// One timed thing.
#[derive(Debug)]
struct Measurement {
    program: String,
    stage: &'static str,
    runs: Vec<Duration>,
}

impl Measurement {
    fn min(&self) -> f64 {
        self.runs
            .iter()
            .map(Duration::as_secs_f64)
            .fold(f64::MAX, f64::min)
            * 1000.0
    }

    fn median(&self) -> f64 {
        let mut ms: Vec<f64> = self.runs.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        // The lower of the two middles for an even count, rather than their mean: an
        // average of two timings is a number that was never observed.
        let middle = ms.len().saturating_sub(1).wrapping_div(2);
        ms.get(middle).copied().unwrap_or(0.0)
    }

    fn key(&self) -> String {
        format!("{}/{}", self.program, self.stage)
    }

    /// Every run, in milliseconds, sorted (N92).
    fn sorted(&self) -> Vec<f64> {
        let mut ms: Vec<f64> = self.runs.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        ms
    }

    /// The nearest-rank 90th percentile: a run that was observed, never an interpolation.
    fn p90(&self) -> f64 {
        let ms = self.sorted();
        let rank = (ms.len() * 9).div_ceil(10).max(1);
        ms.get(rank - 1).copied().unwrap_or(0.0)
    }

    fn max(&self) -> f64 {
        self.sorted().last().copied().unwrap_or(0.0)
    }

    /// The median absolute deviation from the median: a spread one slow run cannot inflate.
    fn mad(&self) -> f64 {
        let median = self.median();
        let mut d: Vec<f64> = self.sorted().iter().map(|x| (x - median).abs()).collect();
        d.sort_by(f64::total_cmp);
        let middle = d.len().saturating_sub(1).wrapping_div(2);
        d.get(middle).copied().unwrap_or(0.0)
    }
}

fn shell(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned())
}

/// Run a command once and time it, requiring success.
fn once(cmd: &mut Command) -> Result<Duration, String> {
    let start = Instant::now();
    let out = cmd.output().map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();
    if out.status.success() {
        Ok(elapsed)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Time a command `runs` times, after one untimed warm-up.
///
/// The warm-up is not cheating: the first run pays for reading the binary and the source
/// off disk, which is a property of the filesystem cache rather than of the compiler.
/// It is discarded and said so rather than quietly folded into the minimum.
fn measure(
    program: &str,
    stage: &'static str,
    runs: usize,
    mut build: impl FnMut() -> Command,
) -> Result<Measurement, String> {
    once(&mut build())?;
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        times.push(once(&mut build())?);
    }
    Ok(Measurement {
        program: program.to_owned(),
        stage,
        runs: times,
    })
}

pub(crate) fn run(mode: Mode, runs: usize) -> ExitCode {
    // Every program here runs as a user runs it (R1, 5b.3): it prints its answer and ends with
    // status 0. `cargo xtask` inherits `.cargo/config.toml`'s result report, which would add
    // `main`'s `0` to what a program prints and to no reference's; it is removed before any
    // program is started, and before any thread exists.
    std::env::remove_var("NAZM_RESULT_REPORT");
    let root = crate::guide::root();
    let nazm = target_dir().join("release/nazm");
    if !nazm.exists() {
        eprintln!(
            "bench: {} is missing. Measure the release build, not the debug one:\n  \
             cargo build --release -p nazm-cli",
            nazm.display()
        );
        return ExitCode::FAILURE;
    }

    let mut programs: Vec<PathBuf> = std::fs::read_dir(root.join("bench/programs"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "nz"))
        .collect();
    programs.sort();
    if programs.is_empty() {
        eprintln!("bench: no programs in bench/programs");
        return ExitCode::FAILURE;
    }

    let scratch = match tempdir(&root) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("bench: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Before any timing: both implementations must produce the recorded answer.
    //
    // `docs/performance.md` rule 6 asks for equivalent work and results to be
    // verified rather than assumed, and this is the failure it is guarding against — a
    // benchmark whose compiled form was folded away, or whose two implementations
    // quietly disagree, still produces a number, and the number is the thing people
    // quote. Refusing to report is the only useful response.
    if let Err(e) = verify(&nazm, &scratch, &programs) {
        eprintln!("bench: {e}");
        return ExitCode::FAILURE;
    }

    let mut measurements = Vec::new();
    for path in &programs {
        let name = path
            .file_stem()
            .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
        let exe = scratch.join(&name);
        let source = path.display().to_string();

        let stages: Vec<Stage> = vec![
            ("check", {
                let (n, s) = (nazm.clone(), source.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["check", &s]);
                    c
                })
            }),
            ("build-O0", {
                let (n, s, e) = (nazm.clone(), source.clone(), exe.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["build", &s, "-o"])
                        .arg(&e)
                        .args(["--opt-level", "0"]);
                    c
                })
            }),
            ("build-O2", {
                let (n, s, e) = (nazm.clone(), source.clone(), exe.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["build", &s, "-o"])
                        .arg(&e)
                        .args(["--opt-level", "2"]);
                    c
                })
            }),
            ("interpret", {
                let (n, s) = (nazm.clone(), source.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["run", "--interpret", &s]);
                    c
                })
            }),
            ("native-O2", {
                let e = exe.clone();
                Box::new(move || Command::new(&e))
            }),
        ];

        for (stage, build) in stages {
            match measure(&name, stage, runs, build) {
                Ok(m) => measurements.push(m),
                Err(e) => {
                    eprintln!("bench: {name}/{stage} failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    // The compiler compiling itself: the largest Nazm program that exists, and the one
    // whose compile time anybody actually waits for.
    match compiler_source(&root, &scratch) {
        Ok(whole) => {
            let source = whole.display().to_string();
            let exe = scratch.join("selfcompile");
            for (stage, args) in [
                ("check", vec!["check".to_owned(), source.clone()]),
                (
                    "build-O0",
                    vec![
                        "build".to_owned(),
                        source.clone(),
                        "-o".to_owned(),
                        exe.display().to_string(),
                        "--opt-level".to_owned(),
                        "0".to_owned(),
                    ],
                ),
            ] {
                let n = nazm.clone();
                match measure("compiler", stage, runs, || {
                    let mut c = Command::new(&n);
                    c.args(&args);
                    c
                }) {
                    Ok(m) => measurements.push(m),
                    Err(e) => {
                        eprintln!("bench: compiler/{stage} failed: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
        Err(e) => eprintln!("bench: skipping the self-compile benchmark: {e}"),
    }

    // One external reference, where the toolchain is present.
    //
    // §12 asks for a justified external reference where locally available, and one is
    // the right number: `bench/reference/sieve.c` and `sieve.py` are the Nazm sieve
    // statement for statement, not the way a C or Python programmer would write it,
    // because a different algorithm would measure the algorithm. Absent toolchains are
    // recorded as absent rather than skipped silently.
    let mut absent = vec![Absent {
        stage: "reference-zig",
        why: "no Zig reference is written: no Zig toolchain is installed here to check one"
            .to_owned(),
    }];
    measurements.extend(references(&root, &nazm, &scratch, runs, &mut absent));
    // Each native executable's size (N92).
    let sizes: Vec<(String, Option<u64>)> = programs
        .iter()
        .map(|p| {
            let name = p
                .file_stem()
                .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
            let bytes = std::fs::metadata(scratch.join(&name)).ok().map(|m| m.len());
            (name, bytes)
        })
        .collect();

    // Peak memory, once per command rather than repeatedly: it varies far less than
    // time does, and `docs/performance.md` rule 2 asks for it separately from
    // speed because they are separate questions.
    let mut memory = Vec::new();
    for path in &programs {
        let name = path
            .file_stem()
            .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
        let exe = scratch.join(&name);
        let source = path.display().to_string();
        let mut interpret = Command::new(&nazm);
        interpret.args(["run", "--interpret", &source]);
        memory.push(Memory {
            program: name.clone(),
            stage: "interpret",
            peak_bytes: peak_rss(&mut interpret),
        });
        memory.push(Memory {
            program: name,
            stage: "native-O2",
            peak_bytes: peak_rss(&mut Command::new(&exe)),
        });
    }

    let json = render_json(&root, runs, &measurements, &memory, &sizes, &absent);
    print!("{}", render_table(&measurements));
    print!("{}", render_memory(&memory));

    let record = bench_dir(&root).join("record.json");
    if let Err(e) = std::fs::write(&record, &json) {
        eprintln!("bench: cannot write {}: {e}", record.display());
        return ExitCode::FAILURE;
    }
    println!("\nwritten: {}", record.display());

    match mode {
        Mode::Record => ExitCode::SUCCESS,
        Mode::Save => {
            let baseline = baseline_path(&root);
            match std::fs::write(&baseline, &json) {
                Ok(()) => {
                    println!("baseline: {}", baseline.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("bench: cannot write {}: {e}", baseline.display());
                    ExitCode::FAILURE
                }
            }
        }
        Mode::Check => compare(&root, &measurements),
    }
}

/// Time the external references, where the toolchain for one is installed.
///
/// Each must produce the same answer as the Nazm program it mirrors, checked against the
/// same `.expected` file — a reference that computes something else is not a reference.
/// The references, and whether each one's arithmetic is checked as Nazm's is (N92).
const REFERENCES: &[(&str, &str, &str)] = &[
    (
        "reference-c",
        "clang -O2",
        "unchecked: C's signed overflow is undefined, not failed",
    ),
    (
        "reference-python",
        "python3",
        "unbounded integers: nothing overflows",
    ),
    (
        "reference-rust",
        "rustc -O -C overflow-checks=on",
        "checked, and bounds-checked: the closest to Nazm's meaning",
    ),
    ("reference-go", "go build", "wrapping, bounds-checked"),
];

/// A reference that was not measured, and why.
struct Absent {
    stage: &'static str,
    why: String,
}

fn references(
    root: &Path,
    nazm: &Path,
    scratch: &Path,
    runs: usize,
    absent: &mut Vec<Absent>,
) -> Vec<Measurement> {
    let mut out = Vec::new();
    let dir = root.join("bench/reference");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut sources: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    sources.sort();

    for source in sources {
        let Some(name) = source.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let expected = root.join("bench/programs").join(format!("{name}.expected"));
        let Ok(expected) = std::fs::read_to_string(&expected) else {
            eprintln!("bench: {name} has no Nazm counterpart to be a reference to");
            continue;
        };
        let expected = expected.trim().to_owned();
        let _ = nazm;

        let (stage, mut command): (&'static str, Box<dyn Fn() -> Command>) = match source
            .extension()
            .and_then(|e| e.to_str())
        {
            Some("c") => {
                let exe = scratch.join(format!("{name}.c.out"));
                let built = Command::new("clang")
                    .arg("-O2")
                    .arg("-o")
                    .arg(&exe)
                    .arg(&source)
                    .output();
                match built {
                    Ok(o) if o.status.success() => {
                        ("reference-c", Box::new(move || Command::new(&exe)))
                    }
                    _ => {
                        println!("bench: clang is unavailable; the C reference was not measured");
                        absent.push(Absent {
                            stage: "reference-c",
                            why: "clang is not installed".to_owned(),
                        });
                        continue;
                    }
                }
            }
            Some("rs") => {
                let exe = scratch.join(format!("{name}.rs.out"));
                let built = Command::new("rustc")
                    .args(["-O", "-C", "overflow-checks=on", "-o"])
                    .arg(&exe)
                    .arg(&source)
                    .output();
                match built {
                    Ok(o) if o.status.success() => {
                        ("reference-rust", Box::new(move || Command::new(&exe)))
                    }
                    _ => {
                        println!(
                            "bench: rustc is unavailable; the Rust reference was not measured"
                        );
                        absent.push(Absent {
                            stage: "reference-rust",
                            why: "rustc could not build it".to_owned(),
                        });
                        continue;
                    }
                }
            }
            Some("go") => {
                let exe = scratch.join(format!("{name}.go.out"));
                // Go's caches in the scratch directory: nothing written outside it.
                let built = Command::new("go")
                    .env("GOCACHE", scratch.join("gocache"))
                    .env("GOPATH", scratch.join("gopath"))
                    .env("GOFLAGS", "-mod=mod")
                    .env("GOTOOLCHAIN", "local")
                    .args(["build", "-o"])
                    .arg(&exe)
                    .arg(&source)
                    .output();
                match built {
                    Ok(o) if o.status.success() => {
                        ("reference-go", Box::new(move || Command::new(&exe)))
                    }
                    _ => {
                        println!("bench: go is unavailable; the Go reference was not measured");
                        absent.push(Absent {
                            stage: "reference-go",
                            why: "go is not installed".to_owned(),
                        });
                        continue;
                    }
                }
            }
            Some("py") => {
                if Command::new("python3").arg("--version").output().is_err() {
                    println!("bench: python3 is unavailable; that reference was not measured");
                    absent.push(Absent {
                        stage: "reference-python",
                        why: "python3 is not installed".to_owned(),
                    });
                    continue;
                }
                let s = source.clone();
                (
                    "reference-python",
                    Box::new(move || {
                        let mut c = Command::new("python3");
                        c.arg(&s);
                        c
                    }),
                )
            }
            _ => continue,
        };

        let got = output(&mut command());
        if got != expected {
            eprintln!(
                "bench: the {stage} for {name} produced {got:?}, expected {expected:?} — \
                 it is not computing the same thing and was not measured"
            );
            absent.push(Absent {
                stage,
                why: format!("it printed {got:?}, not {expected:?}"),
            });
            continue;
        }
        match measure(&name, stage, runs, &mut *command) {
            Ok(m) => out.push(m),
            Err(e) => eprintln!("bench: {name}/{stage} failed: {e}"),
        }
    }
    out
}

/// Every program must produce its recorded answer, interpreted and compiled.
///
/// The `.expected` file beside each program is the same convention `nazm test` and
/// `compiler/bootstrap.sh` use. A program with none is refused rather than skipped: an
/// unverified benchmark is a number nobody can defend, and skipping it silently is how
/// it becomes one.
fn verify(nazm: &Path, scratch: &Path, programs: &[PathBuf]) -> Result<(), String> {
    for path in programs {
        let name = path
            .file_stem()
            .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
        let expected_path = path.with_extension("expected");
        let expected = std::fs::read_to_string(&expected_path)
            .map_err(|_| {
                format!(
                    "{} has no `.expected` beside it. Every benchmark states its answer,                      so that a compiled form which folded away cannot be timed as though                      it had run.",
                    path.display()
                )
            })?
            .trim()
            .to_owned();

        let interpreted =
            output(Command::new(nazm).args(["run", "--interpret", &path.display().to_string()]));
        if interpreted != expected {
            return Err(format!(
                "{name}: `nazm run` produced {interpreted:?}, expected {expected:?}"
            ));
        }

        let exe = scratch.join(&name);
        let built = Command::new(nazm)
            .args(["build", &path.display().to_string(), "-o"])
            .arg(&exe)
            .args(["--opt-level", "2"])
            .output()
            .map_err(|e| format!("{name}: could not build: {e}"))?;
        if !built.status.success() {
            return Err(format!(
                "{name}: `nazm build` refused it: {}",
                String::from_utf8_lossy(&built.stderr).trim()
            ));
        }
        let native = output(&mut Command::new(&exe));
        if native != expected {
            return Err(format!(
                "{name}: the -O2 executable produced {native:?}, expected {expected:?}"
            ));
        }
    }
    Ok(())
}

fn output(cmd: &mut Command) -> String {
    cmd.output().map_or_else(
        |e| format!("could not run: {e}"),
        |o| {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            text.trim().to_owned()
        },
    )
}

/// The compiler's own source, as the bootstrap chain compiles it: the root, with its
/// imports resolved on the way in.
///
/// This used to paste the sources together and strip their `use` lines, which is what
/// `compiler/bootstrap.sh` did at the time. Both now resolve instead, and this keeps no
/// list of its own — a second copy of which files the compiler is made of is a second
/// thing to forget to update, and forgetting it is exactly what happened: `module.nz` was
/// added, the list was not, and the benchmark measured a program missing the function
/// that resolves imports.
fn compiler_source(root: &Path, _scratch: &Path) -> Result<PathBuf, String> {
    let path = root.join("compiler").join("emit.nz");
    if !path.exists() {
        return Err(format!("{} is missing", path.display()));
    }
    Ok(path)
}

fn tempdir(_root: &Path) -> Result<PathBuf, String> {
    let dir = target_dir().join("bench-scratch");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn render_table(measurements: &[Measurement]) -> String {
    let mut out = String::new();
    let width = measurements
        .iter()
        .map(|m| m.key().len())
        .max()
        .unwrap_or(20);
    let _ = writeln!(
        out,
        "\n  {:<width$}  {:>10}  {:>10}",
        "", "min ms", "median ms"
    );
    for m in measurements {
        let _ = writeln!(
            out,
            "  {:<width$}  {:>10.1}  {:>10.1}",
            m.key(),
            m.min(),
            m.median()
        );
    }
    out
}

fn render_memory(memory: &[Memory]) -> String {
    let mut out = String::new();
    let width = memory
        .iter()
        .map(|m| m.program.len() + m.stage.len() + 1)
        .max()
        .unwrap_or(20);
    let _ = writeln!(out, "\n  {:<width$}  {:>12}", "peak memory", "bytes");
    for m in memory {
        let key = format!("{}/{}", m.program, m.stage);
        match m.peak_bytes {
            Some(b) => {
                let _ = writeln!(out, "  {key:<width$}  {b:>12}");
            }
            None => {
                let _ = writeln!(out, "  {key:<width$}  {:>12}", "not measured");
            }
        }
    }
    out
}

/// The machine, as far as it can be read without privileges (N92): CPU model, logical CPUs,
/// memory in bytes, kernel release.
fn fingerprint() -> (String, usize, Option<u64>, String) {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let line = |key: &str| {
        cpuinfo
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_owned())
    };
    let model = if cpuinfo.is_empty() {
        shell("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        line("model name").unwrap_or_else(|| {
            format!(
                "implementer {} part {}",
                line("CPU implementer").unwrap_or_default(),
                line("CPU part").unwrap_or_default()
            )
        })
    };
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let memory = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|m| {
            m.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        })
        .or_else(|| shell("sysctl", &["-n", "hw.memsize"]).parse().ok());
    (model, cpus, memory, shell("uname", &["-r"]))
}

fn render_json(
    root: &Path,
    runs: usize,
    measurements: &[Measurement],
    memory: &[Memory],
    sizes: &[(String, Option<u64>)],
    absent: &[Absent],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"schema\": \"{SCHEMA}\",");
    let _ = writeln!(out, "  \"runs\": {runs},");
    let _ = writeln!(out, "  \"warmups_discarded\": 1,");
    let _ = writeln!(out, "  \"host\": {},", quote(&shell("uname", &["-sm"])));
    let (model, cpus, memory_bytes, kernel) = fingerprint();
    let _ = writeln!(out, "  \"cpus\": \"{cpus}\",");
    let _ = writeln!(out, "  \"cpu_model\": {},", quote(&model));
    let _ = writeln!(
        out,
        "  \"memory_bytes\": {},",
        memory_bytes.map_or_else(|| "null".to_owned(), |b| b.to_string())
    );
    let _ = writeln!(out, "  \"kernel\": {},", quote(&kernel));
    let _ = writeln!(
        out,
        "  \"rustc\": {},",
        quote(&shell("rustc", &["--version"]))
    );
    let _ = writeln!(
        out,
        "  \"clang\": {},",
        quote(shell("clang", &["--version"]).lines().next().unwrap_or(""))
    );
    let _ = writeln!(
        out,
        "  \"commit\": {},",
        quote(&shell(
            "git",
            &["-C", &root.display().to_string(), "rev-parse", "HEAD"]
        ))
    );
    let clean = shell(
        "git",
        &["-C", &root.display().to_string(), "status", "--porcelain"],
    )
    .is_empty();
    let _ = writeln!(out, "  \"tree_clean\": {clean},");
    let _ = writeln!(
        out,
        "  \"not_controlled\": [\"thermal state\", \"other processes\", \"CPU frequency \
         scaling\", \"address-space layout\"],"
    );
    let _ = writeln!(out, "  \"measurements\": [");
    for (i, m) in measurements.iter().enumerate() {
        let comma = if i + 1 == measurements.len() { "" } else { "," };
        let all: Vec<String> = m.sorted().iter().map(|x| format!("{x:.3}")).collect();
        let _ = writeln!(
            out,
            "    {{\"program\": {}, \"stage\": {}, \"min_ms\": {:.3}, \"median_ms\": {:.3}, \"p90_ms\": {:.3}, \"max_ms\": {:.3}, \"mad_ms\": {:.3}, \"runs_ms\": [{}]}}{comma}",
            quote(&m.program),
            quote(m.stage),
            m.min(),
            m.median(),
            m.p90(),
            m.max(),
            m.mad(),
            all.join(", ")
        );
    }
    let _ = writeln!(out, "  ],");
    // The floor (N92): the median of a program that does nothing, natively — process start-up
    // and exit. No difference smaller than it is claimed.
    let floor = measurements
        .iter()
        .find(|m| m.program == "floor" && m.stage == "native-O2")
        .map_or_else(|| "null".to_owned(), |m| format!("{:.3}", m.median()));
    let _ = writeln!(out, "  \"noise_floor_ms\": {floor},");
    let _ = writeln!(out, "  \"binary_bytes\": [");
    for (i, (program, bytes)) in sizes.iter().enumerate() {
        let comma = if i + 1 == sizes.len() { "" } else { "," };
        let b = bytes.map_or_else(|| "null".to_owned(), |b| b.to_string());
        let _ = writeln!(
            out,
            "    {{\"program\": {}, \"stage\": \"native-O2\", \"bytes\": {b}}}{comma}",
            quote(program)
        );
    }
    let _ = writeln!(out, "  ],");
    let _ = writeln!(out, "  \"references\": [");
    for (i, (stage, how, arithmetic)) in REFERENCES.iter().enumerate() {
        let comma = if i + 1 == REFERENCES.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"stage\": {}, \"built_with\": {}, \"arithmetic\": {}}}{comma}",
            quote(stage),
            quote(how),
            quote(arithmetic)
        );
    }
    let _ = writeln!(out, "  ],");
    let _ = writeln!(out, "  \"not_measured\": [");
    for (i, a) in absent.iter().enumerate() {
        let comma = if i + 1 == absent.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"stage\": {}, \"why\": {}}}{comma}",
            quote(a.stage),
            quote(&a.why)
        );
    }
    let _ = writeln!(out, "  ],");
    // `null` rather than 0 for a peak that could not be read: a measurement that did not
    // happen and one that came out at zero are different facts.
    let _ = writeln!(
        out,
        "  \"peak_memory_source\": \"/usr/bin/time -l, bytes on Darwin\","
    );
    let _ = writeln!(out, "  \"peak_memory\": [");
    for (i, m) in memory.iter().enumerate() {
        let comma = if i + 1 == memory.len() { "" } else { "," };
        let bytes = m
            .peak_bytes
            .map_or_else(|| "null".to_owned(), |b| b.to_string());
        let _ = writeln!(
            out,
            "    {{\"program\": {}, \"stage\": {}, \"peak_bytes\": {bytes}}}{comma}",
            quote(&m.program),
            quote(m.stage)
        );
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

fn quote(s: &str) -> String {
    let escaped: String = s
        .chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            other => vec![other],
        })
        .collect();
    format!("\"{escaped}\"")
}

/// The baseline belonging to *this* machine.
///
/// `bench/baseline.json` was recorded on Darwin. A Linux run must not overwrite it: the
/// comparison already refuses to run across hosts, so one file per host is the only
/// arrangement in which both can exist and either can be checked. Whichever host owns
/// the legacy name keeps it, so nothing has to be migrated, and every other host gets
/// `bench/baseline-<host>.json`.
fn baseline_path(root: &Path) -> PathBuf {
    let host = shell("uname", &["-sm"]);
    let legacy = bench_dir(root).join("baseline.json");
    match std::fs::read_to_string(&legacy) {
        Ok(text) if field(&text, "host") == host => legacy,
        _ => {
            let slug: String = host
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            bench_dir(root).join(format!("baseline-{slug}.json"))
        }
    }
}

/// Compare against the recorded baseline, on the same machine only.
fn compare(root: &Path, measurements: &[Measurement]) -> ExitCode {
    let path = baseline_path(root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!(
            "bench: no baseline at {}. Record one with `cargo xtask bench --save`.",
            path.display()
        );
        return ExitCode::FAILURE;
    };

    let baseline_host = field(&text, "host");
    let host = shell("uname", &["-sm"]);
    if baseline_host != host {
        // Not a failure. A percentage between two machines is a number with no meaning,
        // and producing one anyway is how performance claims become folklore.
        println!(
            "\nbench: the baseline was recorded on {baseline_host:?} and this is \
             {host:?}; not comparing."
        );
        return ExitCode::SUCCESS;
    }

    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    for m in measurements {
        let Some(was) = baseline_median(&text, &m.program, m.stage) else {
            println!("bench: {} is not in the baseline", m.key());
            continue;
        };
        let now = m.median();
        if was <= 0.0 {
            continue;
        }
        let ratio = now / was;
        let moved = (now - was).abs();
        if moved < FLOOR_MS {
            continue;
        }
        if ratio > 1.0 + TOLERANCE {
            regressions.push(format!("{}: {was:.1} -> {now:.1} ms", m.key()));
        } else if ratio < 1.0 - TOLERANCE {
            improvements.push(format!("{}: {was:.1} -> {now:.1} ms", m.key()));
        }
    }

    // A benchmark in the baseline that this run did not produce. Deleting a program is
    // a legitimate thing to do; doing it and having the comparison quietly cover one
    // fewer thing is not, and silence looks the same either way.
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("{\"program\": \"") else {
            continue;
        };
        let Some((program, rest)) = rest.split_once('"') else {
            continue;
        };
        let Some(stage) = rest
            .split("\"stage\": \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        else {
            continue;
        };
        if !measurements
            .iter()
            .any(|m| m.program == program && m.stage == stage)
        {
            println!("bench: {program}/{stage} is in the baseline and was not measured");
        }
    }

    for i in &improvements {
        println!("faster: {i}");
    }
    if regressions.is_empty() {
        println!(
            "\nbench: nothing slower than the baseline by more than {:.0}% and {FLOOR_MS:.0} ms",
            TOLERANCE * 100.0
        );
        return ExitCode::SUCCESS;
    }
    for r in &regressions {
        eprintln!("slower: {r}");
    }
    eprintln!(
        "\nbench: {} measurement(s) slower than the baseline by more than {:.0}% and \
         {FLOOR_MS:.0} ms",
        regressions.len(),
        TOLERANCE * 100.0
    );
    ExitCode::FAILURE
}

/// A string field from the baseline. A hand-rolled read rather than a JSON parser: the
/// file is one this program wrote, and `xtask` has no business gaining a dependency to
/// read its own output.
fn field(text: &str, name: &str) -> String {
    text.lines()
        .find_map(|l| {
            let l = l.trim();
            let prefix = format!("\"{name}\": \"");
            l.strip_prefix(&prefix)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn baseline_median(text: &str, program: &str, stage: &str) -> Option<f64> {
    let needle = format!("\"program\": \"{program}\", \"stage\": \"{stage}\"");
    // The value runs to the next field or the end of the object: `/1` ended there, `/2`
    // continues with `p90_ms`.
    text.lines()
        .find(|l| l.contains(&needle))?
        .split("\"median_ms\": ")
        .nth(1)?
        .split([',', '}'])
        .next()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(ms: &[u64]) -> Measurement {
        Measurement {
            program: "p".to_owned(),
            stage: "s",
            runs: ms.iter().map(|m| Duration::from_millis(*m)).collect(),
        }
    }

    #[test]
    fn the_spread_is_of_runs_that_were_observed() {
        let m = runs(&[12, 10, 11, 50, 10, 13, 11]);
        assert!((m.min() - 10.0).abs() < 1e-9);
        assert!((m.median() - 11.0).abs() < 1e-9);
        // Nearest rank: the 7th of 7 sorted runs is the 90th percentile.
        assert!((m.p90() - 50.0).abs() < 1e-9);
        assert!((m.max() - 50.0).abs() < 1e-9);
        // One slow run moves the maximum, not the median absolute deviation.
        assert!((m.mad() - 1.0).abs() < 1e-9);
        assert!((runs(&[5]).p90() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_baseline_of_either_version_is_read() {
        let v1 = "{\"program\": \"sieve\", \"stage\": \"native-O2\", \"min_ms\": 1.000, \"median_ms\": 2.500},";
        let v2 = "{\"program\": \"sieve\", \"stage\": \"native-O2\", \"min_ms\": 1.000, \"median_ms\": 2.500, \"p90_ms\": 3.000, \"max_ms\": 4.000, \"mad_ms\": 0.100, \"runs_ms\": [1.000, 2.500]}";
        for text in [v1, v2] {
            assert_eq!(baseline_median(text, "sieve", "native-O2"), Some(2.5));
        }
    }
}
