//! `nazm test` — run a corpus of programs against their recorded outputs.
//!
//! # The convention, which already existed
//!
//! A test case is `NAME.nz` beside `NAME.expected`. `compiler/bootstrap.sh` has compared
//! the conformance corpus that way since the bootstrap existed; this makes the same
//! comparison a command, so it is available to anything that is not the bootstrap
//! script — a person, an agent, or a `--json` consumer.
//!
//! # Both implementations, every time
//!
//! Each case runs under `nazm run` **and** as a `nazm build` executable, and both must
//! produce the recorded output. Running only one would make the corpus a test of that
//! one, and the thing most worth catching here is the two of them disagreeing.
//!
//! # A `.nz` with no `.expected` is counted, not ignored
//!
//! It is not a test case — a directory may hold a helper — but the number of them is
//! reported. A silently skipped file is how a renamed test stops running and nobody
//! notices.
//!
//! # Test functions (Gate 2, §18)
//!
//! Every `@test` and `@fuzz` function a file marks is a case too, `STEM::NAME`, run through the
//! generated entry [`crate::harness`] writes: under `nazm run` and as an executable, each in its
//! own process. It passes when the process ends `0` — the function answered `Ok` — and a
//! `@test(fails)` passes when it ends `2`, the status of an `Err` or a trap; a program that does
//! not check passes neither. `--filter` keeps the cases whose name contains it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::exit;
use crate::harness::{Harness, Kind};

/// What a case must do to pass.
#[derive(Debug, Clone)]
enum Expect {
    /// Print what its `.expected` file holds.
    Output(String),
    /// End `0`: a test function that answered `Ok`.
    Passes,
    /// End `2`: a `@test(fails)` that answered `Err` or trapped.
    Fails,
}

/// One case, before it runs.
#[derive(Debug, Clone)]
struct Planned {
    name: String,
    file: PathBuf,
    expect: Expect,
    /// The marked function a test case runs through the harness; `None` for a recorded output.
    harness: Option<Harness>,
}

/// `nazm test --fuzz`'s default: how many inputs a `@fuzz` target is given.
pub(crate) const DEFAULT_FUZZ_CASES: u32 = 100;

/// The version every `nazm test --json` object carries. See `docs/diagnostics.md`.
const SCHEMA: &str = "nazm.test/1";

#[derive(Debug, serde::Serialize)]
struct Case {
    schema: &'static str,
    name: String,
    file: String,
    /// `golden` for a recorded output, `test` or `fuzz` for a marked function (Gate 2).
    kind: &'static str,
    /// The recorded output; `passes` or `fails` for a marked function.
    expected: String,
    /// What `nazm run` produced, and whether it matched.
    interpreted: Outcome,
    /// What the compiled executable produced. `None` when the native leg was not run.
    native: Option<Outcome>,
    passed: bool,
    /// How each leg ended, as the runner observed it — structure `--summary-json` reports
    /// (N31) and `nazm.test/1` does not carry.
    #[serde(skip)]
    legs: (Leg, Leg),
}

#[derive(Debug, serde::Serialize)]
struct Outcome {
    output: String,
    matched: bool,
}

/// How one leg of a case ended: read from what the runner did — which process it started,
/// whether it could, and whether that process succeeded — never from any output's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Leg {
    /// Its output was the recorded output.
    Matched,
    /// It ran, and its output was not the recorded output.
    Mismatched,
    /// Not asked for: `--interpret-only`.
    NotRun,
    /// `nazm build` ran and did not succeed, so there was no executable to run.
    BuildFailed,
    /// `nazm build` could not be started.
    CouldNotBuild,
    /// The program could not be started.
    CouldNotRun,
}

/// The version `nazm test --summary-json` carries (N31).
const SUMMARY_SCHEMA: &str = "nazm.test-summary/1";

/// One run, compactly: counts, and every case that did not pass.
#[derive(Debug, serde::Serialize)]
struct Summary<'a> {
    schema: &'static str,
    /// `passed`, `failed`, `no_cases`, `unreadable` or `toolchain_failed`.
    status: &'static str,
    exit_code: u8,
    /// Whether each case also ran as a compiled executable.
    native: bool,
    counts: Counts,
    /// In the order the cases ran, which is the order they were named and, within a
    /// directory, sorted: never completion order.
    failures: Vec<Failure<'a>>,
}

#[derive(Debug, serde::Serialize)]
struct Counts {
    cases: usize,
    passed: usize,
    failed: usize,
    /// `.nz` files with no `.expected` beside them: not cases, and counted.
    without_expectation: usize,
}

#[derive(Debug, serde::Serialize)]
struct Failure<'a> {
    /// As `nazm.test/1` names the case.
    name: &'a str,
    file: &'a str,
    interpreted: Leg,
    native: Leg,
}

pub(crate) fn run(
    paths: &[PathBuf],
    json: bool,
    interpret_only: bool,
    summary: bool,
    filter: Option<&str>,
    fuzz: u32,
) -> ExitCode {
    // `--summary-json` (N31): at every exit below, the outcome as one `nazm.test-summary/1`
    // on stdout, and nothing on stderr. The exit status is the same.
    let summarized = |status, code: u8, results: &[Case], without: usize| {
        print_summary(status, code, !interpret_only, results, without);
        ExitCode::from(code)
    };
    let nazm = match driver() {
        Ok(p) => p,
        Err(e) => {
            if summary {
                return summarized("toolchain_failed", exit::TOOLCHAIN_FAILED, &[], 0);
            }
            eprintln!("nazm: cannot find this executable: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };

    let mut cases = Vec::new();
    let mut without_expectation = Vec::new();
    for path in paths {
        if let Err(e) = gather(path, fuzz, &mut cases, &mut without_expectation) {
            if summary {
                return summarized(
                    "unreadable",
                    exit::NO_SUCH_FILE,
                    &[],
                    without_expectation.len(),
                );
            }
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    }
    if let Some(f) = filter {
        cases.retain(|c| c.name.contains(f));
    }
    if cases.is_empty() {
        if summary {
            return summarized(
                "no_cases",
                exit::NO_SUCH_FILE,
                &[],
                without_expectation.len(),
            );
        }
        eprintln!(
            "nazm: no test cases. A case is `NAME.nz` beside `NAME.expected`, or a `@test` or \
             `@fuzz` function{}.",
            filter.map_or_else(String::new, |f| format!(" whose name contains `{f}`"))
        );
        return ExitCode::from(exit::NO_SUCH_FILE);
    }

    let dir = match tempdir() {
        Ok(d) => d,
        Err(e) => {
            if summary {
                return summarized(
                    "toolchain_failed",
                    exit::TOOLCHAIN_FAILED,
                    &[],
                    without_expectation.len(),
                );
            }
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };

    let mut results = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        results.push(one(&nazm, &dir, i, case, interpret_only));
    }
    // The executables were only ever needed for the comparison above. Left behind they
    // accumulate one directory per process id, which is a small mess that becomes a
    // confusing one the first time somebody finds a stale binary there and wonders
    // which run wrote it. A failure to remove is not worth reporting: the next run
    // writes over it.
    drop(std::fs::remove_dir_all(&dir));

    let failed = results.iter().filter(|c| !c.passed).count();
    if summary {
        return if failed == 0 {
            summarized("passed", exit::OK, &results, without_expectation.len())
        } else {
            summarized(
                "failed",
                exit::CHECK_FAILED,
                &results,
                without_expectation.len(),
            )
        };
    }
    report(&results, &without_expectation, failed, json);
    if failed == 0 {
        ExitCode::from(exit::OK)
    } else {
        ExitCode::from(exit::CHECK_FAILED)
    }
}

/// The variable naming another `nazm` for each case's legs to start. A test seam, not an
/// interface: it lets a test reach the runner's own could-not-start branches — a build or a
/// program that cannot be started — through a driver of its choosing, without touching the
/// machine's toolchain.
const DRIVER: &str = "NAZM_TEST_DRIVER";

/// The `nazm` each case's legs start: this executable, unless [`DRIVER`] names another.
fn driver() -> std::io::Result<PathBuf> {
    match std::env::var_os(DRIVER) {
        Some(p) => Ok(PathBuf::from(p)),
        None => std::env::current_exe(),
    }
}

/// One `nazm.test-summary/1`: counts from each case's own verdict, and every case that did
/// not pass with how each of its legs ended. No passed name and no output.
fn print_summary(
    status: &'static str,
    exit_code: u8,
    native: bool,
    results: &[Case],
    without: usize,
) {
    let summary = Summary {
        schema: SUMMARY_SCHEMA,
        status,
        exit_code,
        native,
        counts: Counts {
            cases: results.len(),
            passed: results.iter().filter(|c| c.passed).count(),
            failed: results.iter().filter(|c| !c.passed).count(),
            without_expectation: without,
        },
        failures: results
            .iter()
            .filter(|c| !c.passed)
            .map(|c| Failure {
                name: &c.name,
                file: &c.file,
                interpreted: c.legs.0,
                native: c.legs.1,
            })
            .collect(),
    };
    match serde_json::to_string(&summary) {
        Ok(line) => println!("{line}"),
        Err(_) => println!("{{\"schema\":\"{SUMMARY_SCHEMA}\",\"error\":\"unserialisable\"}}"),
    }
}

/// Collect the cases of a file or a directory: a recorded output, and every marked function.
fn gather(
    path: &Path,
    fuzz: u32,
    cases: &mut Vec<Planned>,
    without: &mut Vec<String>,
) -> Result<(), String> {
    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "nz"))
            .collect();
        found.sort();
        for p in found {
            gather(&p, fuzz, cases, without)?;
        }
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("{} is not a file or a directory", path.display()));
    }
    let name = path
        .file_stem()
        .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
    let before = cases.len();
    let expected_path = path.with_extension("expected");
    if let Ok(expected) = std::fs::read_to_string(&expected_path) {
        cases.push(Planned {
            name: name.clone(),
            file: path.to_path_buf(),
            expect: Expect::Output(expected),
            harness: None,
        });
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    for m in crate::harness::marked(path, &text) {
        let kind = match m.attribute {
            "test" => Kind::Test,
            "fuzz" => Kind::Fuzz(fuzz),
            _ => continue,
        };
        cases.push(Planned {
            name: format!("{name}::{}", m.function),
            file: path.to_path_buf(),
            expect: if m.fails {
                Expect::Fails
            } else {
                Expect::Passes
            },
            harness: Some(Harness {
                kind,
                function: m.function,
            }),
        });
    }
    if cases.len() == before {
        without.push(path.display().to_string());
    }
    Ok(())
}

/// Somewhere to put the compiled executables, removed when the run ends.
///
/// Named by process id so two concurrent runs do not write over each other, and
/// **created fresh**: a directory left by a previous run with the same id would hand
/// this one a stale executable for a case that failed to build.
fn tempdir() -> Result<PathBuf, String> {
    // Exclusive and private (Q1-A1-02): an executable is built here and then run.
    crate::scratch::temporary("nazm-test-").map_err(|e| format!("a scratch directory: {e}"))
}

/// Combined output, trimmed — the same comparison `compiler/bootstrap.sh` makes.
///
/// Combined because a program's diagnostic goes to stderr and is as much of its
/// behaviour as its value; trimmed because a trailing newline is a property of
/// `printf`, not of the program.
fn output_of(cmd: &mut Command) -> (String, bool, Option<i32>) {
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (text.trim().to_owned(), true, out.status.code())
        }
        Err(e) => (format!("could not run: {e}"), false, None),
    }
}

/// Whether a leg that printed `got` and ended with `status` did what `expect` asks.
fn met(expect: &Expect, got: &str, status: Option<i32>) -> bool {
    match expect {
        Expect::Output(want) => got == want.trim(),
        Expect::Passes => status == Some(0),
        Expect::Fails => status == Some(i32::from(exit::RUNTIME_FAILED)),
    }
}

/// A leg's ending, from whether its process started and whether its output matched.
const fn leg(ran: bool, matched: bool) -> Leg {
    match (ran, matched) {
        (false, _) => Leg::CouldNotRun,
        (true, true) => Leg::Matched,
        (true, false) => Leg::Mismatched,
    }
}

fn one(nazm: &Path, dir: &Path, index: usize, case: &Planned, interpret_only: bool) -> Case {
    let file = case.file.as_path();
    let harness: Vec<String> = case
        .harness
        .iter()
        .flat_map(|h| ["--harness".to_owned(), h.spec()])
        .collect();
    let want = match &case.expect {
        Expect::Output(w) => w.trim().to_owned(),
        Expect::Passes => "passes".to_owned(),
        Expect::Fails => "fails".to_owned(),
    };

    // A case's `.expected` is what it prints and then `main`'s result, as every one was written
    // before R1 (5b.3): both legs run with the result report, whatever the caller's environment.
    let (got, ran, status) = output_of(
        Command::new(nazm)
            .arg("run")
            .args(&harness)
            .arg(file)
            .env(nazm_lir::RESULT_VARIABLE, "1"),
    );
    let interpreted = Outcome {
        matched: met(&case.expect, &got, status),
        output: got,
    };
    let interpreted_leg = leg(ran, interpreted.matched);
    let mut native_leg = Leg::NotRun;

    let native = if interpret_only {
        None
    } else {
        // By position, not name: `a::t` and `a.nz`'s own case must not share an executable.
        let exe = dir.join(format!("case{index}"));
        let built = Command::new(nazm)
            .arg("build")
            .args(&harness)
            .arg(file)
            .arg("-o")
            .arg(&exe)
            .output();
        match built {
            Ok(out) if out.status.success() => {
                let (got, ran, status) =
                    output_of(Command::new(&exe).env(nazm_lir::RESULT_VARIABLE, "1"));
                let matched = met(&case.expect, &got, status);
                native_leg = leg(ran, matched);
                Some(Outcome {
                    matched,
                    output: got,
                })
            }
            Ok(out) => {
                native_leg = Leg::BuildFailed;
                Some(Outcome {
                    matched: false,
                    output: format!(
                        "`nazm build` refused it: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                })
            }
            Err(e) => {
                native_leg = Leg::CouldNotBuild;
                Some(Outcome {
                    matched: false,
                    output: format!("could not build: {e}"),
                })
            }
        }
    };

    let passed = interpreted.matched && native.as_ref().is_none_or(|n| n.matched);
    Case {
        schema: SCHEMA,
        name: case.name.clone(),
        file: file.display().to_string(),
        kind: match case.harness.as_ref().map(|h| h.kind) {
            None => "golden",
            Some(Kind::Fuzz(_)) => "fuzz",
            Some(_) => "test",
        },
        expected: want,
        interpreted,
        native,
        passed,
        legs: (interpreted_leg, native_leg),
    }
}

fn report(results: &[Case], without: &[String], failed: usize, json: bool) {
    if json {
        for c in results {
            match serde_json::to_string(c) {
                Ok(line) => println!("{line}"),
                Err(_) => println!("{{\"schema\":\"{SCHEMA}\",\"error\":\"unserialisable\"}}"),
            }
        }
    } else {
        for c in results {
            if c.passed {
                println!("ok    {}", c.name);
                continue;
            }
            println!("FAIL  {}  ({})", c.name, c.file);
            println!("      expected: {}", c.expected);
            if !c.interpreted.matched {
                println!("      nazm run: {}", c.interpreted.output);
            }
            if let Some(n) = &c.native {
                if !n.matched {
                    println!("      native:   {}", n.output);
                }
            }
        }
    }

    // Always on stderr, so it survives `--json` being piped into something.
    for p in without {
        eprintln!("nazm: {p} has no `.expected` beside it and is not a test case");
    }
    eprintln!(
        "nazm: {} of {} passed{}",
        results.len() - failed,
        results.len(),
        if without.is_empty() {
            String::new()
        } else {
            format!(", {} file(s) skipped", without.len())
        }
    );
}
