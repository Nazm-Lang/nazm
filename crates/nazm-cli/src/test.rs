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

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::exit;

/// The version every `nazm test --json` object carries. See `docs/diagnostics.md`.
const SCHEMA: &str = "nazm.test/1";

#[derive(Debug, serde::Serialize)]
struct Case {
    schema: &'static str,
    name: String,
    file: String,
    expected: String,
    /// What `nazm run` produced, and whether it matched.
    interpreted: Outcome,
    /// What the compiled executable produced. `None` when the native leg was not run.
    native: Option<Outcome>,
    passed: bool,
}

#[derive(Debug, serde::Serialize)]
struct Outcome {
    output: String,
    matched: bool,
}

pub(crate) fn run(paths: &[PathBuf], json: bool, interpret_only: bool) -> ExitCode {
    let nazm = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("nazm: cannot find this executable: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };

    let mut cases = Vec::new();
    let mut without_expectation = Vec::new();
    for path in paths {
        if let Err(e) = gather(path, &mut cases, &mut without_expectation) {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    }
    if cases.is_empty() {
        eprintln!("nazm: no test cases. A case is `NAME.nz` beside `NAME.expected`.");
        return ExitCode::from(exit::NO_SUCH_FILE);
    }

    let dir = match tempdir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };

    let mut results = Vec::new();
    for (name, file, expected) in cases {
        results.push(one(&nazm, &dir, &name, &file, &expected, interpret_only));
    }

    let failed = results.iter().filter(|c| !c.passed).count();
    report(&results, &without_expectation, failed, json);
    if failed == 0 {
        ExitCode::from(exit::OK)
    } else {
        ExitCode::from(exit::CHECK_FAILED)
    }
}

/// Collect `(name, file, expected)` from a file or a directory.
fn gather(
    path: &Path,
    cases: &mut Vec<(String, PathBuf, String)>,
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
            gather(&p, cases, without)?;
        }
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("{} is not a file or a directory", path.display()));
    }
    let name = path
        .file_stem()
        .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
    let expected_path = path.with_extension("expected");
    match std::fs::read_to_string(&expected_path) {
        Ok(expected) => cases.push((name, path.to_path_buf(), expected)),
        Err(_) => without.push(path.display().to_string()),
    }
    Ok(())
}

fn tempdir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("nazm-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Combined output, trimmed — the same comparison `compiler/bootstrap.sh` makes.
///
/// Combined because a program's diagnostic goes to stderr and is as much of its
/// behaviour as its value; trimmed because a trailing newline is a property of
/// `printf`, not of the program.
fn output_of(cmd: &mut Command) -> String {
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            text.trim().to_owned()
        }
        Err(e) => format!("could not run: {e}"),
    }
}

fn one(
    nazm: &Path,
    dir: &Path,
    name: &str,
    file: &Path,
    expected: &str,
    interpret_only: bool,
) -> Case {
    let want = expected.trim().to_owned();

    let got = output_of(Command::new(nazm).arg("run").arg(file));
    let interpreted = Outcome {
        matched: got == want,
        output: got,
    };

    let native = if interpret_only {
        None
    } else {
        let exe = dir.join(name);
        let built = Command::new(nazm)
            .arg("build")
            .arg(file)
            .arg("-o")
            .arg(&exe)
            .output();
        match built {
            Ok(out) if out.status.success() => {
                let got = output_of(&mut Command::new(&exe));
                Some(Outcome {
                    matched: got == want,
                    output: got,
                })
            }
            Ok(out) => Some(Outcome {
                matched: false,
                output: format!(
                    "`nazm build` refused it: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            }),
            Err(e) => Some(Outcome {
                matched: false,
                output: format!("could not build: {e}"),
            }),
        }
    };

    let passed = interpreted.matched && native.as_ref().is_none_or(|n| n.matched);
    Case {
        schema: SCHEMA,
        name: name.to_owned(),
        file: file.display().to_string(),
        expected: want,
        interpreted,
        native,
        passed,
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
