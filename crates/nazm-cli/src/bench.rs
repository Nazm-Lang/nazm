//! `nazm bench` — measure every `@bench` function of the files named (Gate 2, §19).
//!
//! Each function is built natively at `-O2` through the harness ([`crate::harness`]), which calls
//! it `--iterations` times between two readings of the monotonic clock and prints the time taken;
//! the interpreter is not used, because its time is not the program's. One `nazm.bench/1` row per
//! function with `--json`, a line each otherwise. A measurement is one run on this machine at this
//! moment: what it is worth beside another is the reader's question, not this command's.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::exit;
use crate::harness::{Harness, Kind};

/// The version every `nazm bench --json` object carries.
const SCHEMA: &str = "nazm.bench/1";

#[derive(Debug, serde::Serialize)]
struct Row {
    schema: &'static str,
    /// `STEM::FUNCTION`.
    name: String,
    file: String,
    iterations: u32,
    total_ns: u64,
    ns_per_iteration: u64,
}

pub(crate) fn run(paths: &[PathBuf], iterations: u32, json: bool) -> ExitCode {
    let iterations = iterations.max(1);
    let nazm = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("nazm: cannot find this executable: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    let dir = match crate::scratch::temporary("nazm-bench-") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nazm: a scratch directory: {e}");
            return ExitCode::from(exit::TOOLCHAIN_FAILED);
        }
    };
    let mut files = Vec::new();
    for p in paths {
        if let Err(e) = gather(p, &mut files) {
            eprintln!("nazm: {e}");
            drop(std::fs::remove_dir_all(&dir));
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    }
    let mut measured = 0;
    let mut failed = 0;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let stem = file
            .file_stem()
            .map_or_else(|| "?".to_owned(), |s| s.to_string_lossy().into_owned());
        for m in crate::harness::marked(file, &text) {
            if m.attribute != "bench" {
                continue;
            }
            let name = format!("{stem}::{}", m.function);
            match one(&nazm, &dir, file, &m.function, iterations) {
                Ok(total_ns) => {
                    measured += 1;
                    let row = Row {
                        schema: SCHEMA,
                        name,
                        file: file.display().to_string(),
                        iterations,
                        total_ns,
                        ns_per_iteration: total_ns.checked_div(u64::from(iterations)).unwrap_or(0),
                    };
                    if json {
                        match serde_json::to_string(&row) {
                            Ok(line) => println!("{line}"),
                            Err(_) => {
                                println!("{{\"schema\":\"{SCHEMA}\",\"error\":\"unserialisable\"}}")
                            }
                        }
                    } else {
                        println!(
                            "{:<40} {:>12} ns/iteration  ({} iterations)",
                            row.name, row.ns_per_iteration, iterations
                        );
                    }
                }
                Err(why) => {
                    failed += 1;
                    eprintln!("nazm: {name}: {why}");
                }
            }
        }
    }
    drop(std::fs::remove_dir_all(&dir));
    if measured == 0 && failed == 0 {
        eprintln!("nazm: no `@bench` functions to measure");
        return ExitCode::from(exit::NO_SUCH_FILE);
    }
    if failed == 0 {
        ExitCode::from(exit::OK)
    } else {
        ExitCode::from(exit::CHECK_FAILED)
    }
}

/// Build `function`'s harness and run it once: the nanoseconds its iterations took together.
fn one(
    nazm: &Path,
    dir: &Path,
    file: &Path,
    function: &str,
    iterations: u32,
) -> Result<u64, String> {
    let harness = Harness {
        kind: Kind::Bench(iterations),
        function: function.to_owned(),
    };
    let exe = dir.join(function);
    let built = Command::new(nazm)
        .arg("build")
        .arg("--opt-level")
        .arg("2")
        .arg("--harness")
        .arg(harness.spec())
        .arg(file)
        .arg("-o")
        .arg(&exe)
        .output()
        .map_err(|e| format!("could not build: {e}"))?;
    if !built.status.success() {
        return Err(format!(
            "`nazm build` refused it: {}",
            String::from_utf8_lossy(&built.stderr).trim()
        ));
    }
    let ran = Command::new(&exe)
        .output()
        .map_err(|e| format!("could not run: {e}"))?;
    let out = String::from_utf8_lossy(&ran.stdout);
    if !ran.status.success() {
        return Err(format!(
            "it stopped: {}{}",
            out.trim(),
            String::from_utf8_lossy(&ran.stderr).trim()
        ));
    }
    out.lines()
        .find_map(|l| l.strip_prefix("nazm-bench: "))
        .and_then(|n| n.split_whitespace().next()?.parse::<u64>().ok())
        .ok_or_else(|| format!("no measurement in its output: {}", out.trim()))
}

/// Every `.nz` file named, or found in a directory named, sorted.
fn gather(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "nz"))
            .collect();
        found.sort();
        files.extend(found);
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("{} is not a file or a directory", path.display()));
    }
    files.push(path.to_path_buf());
    Ok(())
}
