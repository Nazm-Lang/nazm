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
const SCHEMA: &str = "nazm.bench/1";

/// How far a median may move before `--check` calls it a change.
///
/// Wide, deliberately. On a laptop with no frequency pinning, run-to-run variation of
/// ten per cent is ordinary, and a gate that fires on ordinary variation is a gate
/// people disable. This is for catching a regression of the kind that matters — a
/// doubling — not for detecting a five per cent win.
const TOLERANCE: f64 = 0.25;

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
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

/// One timed thing.
#[derive(Debug)]
struct Measurement {
    program: String,
    stage: &'static str,
    runs: Vec<Duration>,
}

impl Measurement {
    fn min(&self) -> f64 {
        self.runs.iter().map(Duration::as_secs_f64).fold(f64::MAX, f64::min) * 1000.0
    }

    fn median(&self) -> f64 {
        let mut ms: Vec<f64> = self.runs.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        // The lower of the two middles for an even count, rather than their mean: an
        // average of two timings is a number that was never observed.
        ms.get(ms.len().saturating_sub(1) / 2).copied().unwrap_or(0.0)
    }

    fn key(&self) -> String {
        format!("{}/{}", self.program, self.stage)
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
    let root = root();
    let nazm = root.join("target/release/nazm");
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

    let mut measurements = Vec::new();
    for path in &programs {
        let name = path
            .file_stem()
            .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
        let exe = scratch.join(&name);
        let source = path.display().to_string();

        let stages: Vec<(&'static str, Box<dyn Fn() -> Command>)> = vec![
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
                    c.args(["build", &s, "-o"]).arg(&e).args(["--opt-level", "0"]);
                    c
                })
            }),
            ("build-O2", {
                let (n, s, e) = (nazm.clone(), source.clone(), exe.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["build", &s, "-o"]).arg(&e).args(["--opt-level", "2"]);
                    c
                })
            }),
            ("interpret", {
                let (n, s) = (nazm.clone(), source.clone());
                Box::new(move || {
                    let mut c = Command::new(&n);
                    c.args(["run", &s]);
                    c
                })
            }),
            ("native-O2", {
                let e = exe.clone();
                Box::new(move || Command::new(&e))
            }),
        ];

        for (stage, build) in stages {
            match measure(&name, stage, runs, || build()) {
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

    let json = render_json(&root, runs, &measurements);
    print!("{}", render_table(&measurements));

    let record = root.join("bench/record.json");
    if let Err(e) = std::fs::write(&record, &json) {
        eprintln!("bench: cannot write {}: {e}", record.display());
        return ExitCode::FAILURE;
    }
    println!("\nwritten: {}", record.display());

    match mode {
        Mode::Record => ExitCode::SUCCESS,
        Mode::Save => {
            let baseline = root.join("bench/baseline.json");
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

/// The compiler's four sources as one file, the way `compiler/bootstrap.sh` assembles
/// them.
fn compiler_source(root: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let mut whole = String::new();
    for name in ["lex.nz", "parse.nz", "analyse.nz", "emit.nz"] {
        let path = root.join("compiler").join(name);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // The `use` lines are dropped, exactly as the bootstrap script drops them: the
        // compiler written in Nazm does not resolve them, and this measures the same
        // translation unit the bootstrap builds.
        for line in text.lines() {
            if !line.trim_start().starts_with("use \"") {
                whole.push_str(line);
                whole.push('\n');
            }
        }
    }
    let out = scratch.join("whole.nz");
    std::fs::write(&out, whole).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}

fn tempdir(root: &Path) -> Result<PathBuf, String> {
    let dir = root.join("target/bench-scratch");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn render_table(measurements: &[Measurement]) -> String {
    let mut out = String::new();
    let width = measurements.iter().map(|m| m.key().len()).max().unwrap_or(20);
    let _ = writeln!(out, "\n  {:<width$}  {:>10}  {:>10}", "", "min ms", "median ms");
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

fn render_json(root: &Path, runs: usize, measurements: &[Measurement]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"schema\": \"{SCHEMA}\",");
    let _ = writeln!(out, "  \"runs\": {runs},");
    let _ = writeln!(out, "  \"warmups_discarded\": 1,");
    let _ = writeln!(out, "  \"host\": {},", quote(&shell("uname", &["-sm"])));
    let _ = writeln!(
        out,
        "  \"cpus\": {},",
        quote(&shell("sysctl", &["-n", "hw.ncpu"]))
    );
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
        let _ = writeln!(
            out,
            "    {{\"program\": {}, \"stage\": {}, \"min_ms\": {:.3}, \"median_ms\": {:.3}}}{comma}",
            quote(&m.program),
            quote(m.stage),
            m.min(),
            m.median()
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

/// Compare against the recorded baseline, on the same machine only.
fn compare(root: &Path, measurements: &[Measurement]) -> ExitCode {
    let path = root.join("bench/baseline.json");
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
        if ratio > 1.0 + TOLERANCE {
            regressions.push(format!("{}: {was:.1} -> {now:.1} ms", m.key()));
        } else if ratio < 1.0 - TOLERANCE {
            improvements.push(format!("{}: {was:.1} -> {now:.1} ms", m.key()));
        }
    }

    for i in &improvements {
        println!("faster: {i}");
    }
    if regressions.is_empty() {
        println!("\nbench: nothing slower than the baseline by more than {:.0}%", TOLERANCE * 100.0);
        return ExitCode::SUCCESS;
    }
    for r in &regressions {
        eprintln!("slower: {r}");
    }
    eprintln!(
        "\nbench: {} measurement(s) slower than the baseline by more than {:.0}%",
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
    text.lines()
        .find(|l| l.contains(&needle))?
        .split("\"median_ms\": ")
        .nth(1)?
        .trim_end_matches(['}', ','])
        .trim()
        .parse()
        .ok()
}
