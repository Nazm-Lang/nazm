//! Automated mutation verification.
//!
//! Injects each catalogued defect, rebuilds, and requires the test suite to fail — then
//! restores and requires it to pass.
//!
//! The important part is not the injection; it is proving the injection **landed**. Twice
//! during development a hand-run replacement matched nothing because `cargo fmt` had
//! rewrapped the target, the suite passed, and that outcome was indistinguishable from a
//! surviving mutation. A mutation run that cannot tell "my test is weak" from "my edit did
//! nothing" produces false confidence in whichever direction is more comfortable.
//!
//! So every injection is checked three ways: the pattern must occur exactly once, the file
//! must differ afterwards, and the mutated tree must still compile. A mutation that does
//! not build is reported as unusable rather than counted as caught — an uncompilable
//! program fails the suite for the wrong reason.
//!
//! **"Compiles" has to include the tests.** An earlier version built only
//! `cargo build --workspace`, which leaves test targets unbuilt. A broken test file then
//! made `cargo test` produce no results at all, `failing_tests` counted zero, and every
//! single mutation reported SURVIVED — nineteen of them, all false. The same class of
//! mistake as the silently-skipped injection, and just as convincing. The baseline is now
//! checked before any mutation runs, and the mutated tree is built with `--tests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One catalogued defect. Every field is required: a mutation that cannot say what it
/// breaks, or why, is not reviewable.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Mutation {
    pub(crate) name: String,
    pub(crate) why: String,
    pub(crate) file: String,
    pub(crate) find: String,
    pub(crate) replace: String,
}

#[derive(Debug, serde::Deserialize)]
struct Catalogue {
    mutation: Vec<Mutation>,
}

pub(crate) enum Verdict {
    /// Injected, compiled, and the suite failed. The defect is covered.
    Caught { failing: usize },
    /// Injected and compiled, but every test still passed. A real gap.
    Survived,
    /// The pattern was absent, or matched more than once. Not a result about the tests.
    NotInjected(String),
    /// The mutated tree does not compile, so the suite would fail for the wrong reason.
    DidNotBuild,
}

pub(crate) fn load(path: &Path) -> Result<Vec<Mutation>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let catalogue: Catalogue =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if catalogue.mutation.is_empty() {
        return Err("catalogue has no [[mutation]] entries".to_owned());
    }
    Ok(catalogue.mutation)
}

pub(crate) fn run_all(root: &Path, catalogue: &Path) -> Result<(), Vec<String>> {
    let mutations = match load(catalogue) {
        Ok(m) => m,
        Err(e) => return Err(vec![e]),
    };

    // Establish a baseline before touching anything. If the suite is already failing, or
    // does not run, every result below is meaningless.
    if !builds(root) {
        return Err(vec![
            "the unmutated workspace does not build (including tests). Fix that first: \
             every mutation would otherwise be reported as SURVIVED."
                .to_owned(),
        ]);
    }
    let baseline_failing = failing_tests(root);
    if baseline_failing > 0 {
        return Err(vec![format!(
            "{baseline_failing} test(s) already fail before any mutation. A mutation \
             cannot be shown to be caught by a suite that is not green."
        )]);
    }
    let baseline_results = result_lines(root);
    if baseline_results == 0 {
        return Err(vec![
            "the test suite produced no results at all, so nothing here would mean \
             anything."
                .to_owned(),
        ]);
    }

    let mut problems = Vec::new();
    for m in &mutations {
        match apply_and_test(root, m) {
            Verdict::Caught { failing } => {
                println!("  caught       {}  ({failing} test(s) failed)", m.name);
            }
            Verdict::Survived => {
                println!("  SURVIVED     {}", m.name);
                problems.push(format!(
                    "{}: injected and compiled, but the suite still passed. {}",
                    m.name, m.why
                ));
            }
            Verdict::NotInjected(reason) => {
                println!("  NOT INJECTED {}", m.name);
                problems.push(format!(
                    "{}: {reason}. A mutation that does not apply proves nothing — fix the \
                     pattern rather than reading the pass as coverage.",
                    m.name
                ));
            }
            Verdict::DidNotBuild => {
                println!("  UNUSABLE     {}", m.name);
                problems.push(format!(
                    "{}: the mutated tree does not compile, so a failing suite would not \
                     mean the defect was detected.",
                    m.name
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn apply_and_test(root: &Path, m: &Mutation) -> Verdict {
    let path: PathBuf = root.join(&m.file);
    let Ok(original) = fs::read_to_string(&path) else {
        return Verdict::NotInjected(format!("cannot read {}", m.file));
    };

    let occurrences = original.matches(&m.find).count();
    if occurrences == 0 {
        return Verdict::NotInjected(format!("pattern not found in {}", m.file));
    }
    if occurrences > 1 {
        return Verdict::NotInjected(format!(
            "pattern occurs {occurrences} times in {}; it must be unique",
            m.file
        ));
    }

    let mutated = original.replacen(&m.find, &m.replace, 1);
    if mutated == original {
        return Verdict::NotInjected("replacement produced an identical file".to_owned());
    }
    if fs::write(&path, &mutated).is_err() {
        return Verdict::NotInjected(format!("cannot write {}", m.file));
    }

    let verdict = if builds(root) {
        match failing_tests(root) {
            0 => Verdict::Survived,
            failing => Verdict::Caught { failing },
        }
    } else {
        Verdict::DidNotBuild
    };

    let _ = fs::write(&path, &original);
    verdict
}

fn builds(root: &Path) -> bool {
    // `--tests` matters: without it a broken test file compiles nothing, `cargo test`
    // reports nothing, and every mutation looks like it survived.
    Command::new(cargo())
        .arg("build")
        .arg("--workspace")
        .arg("--tests")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}

/// How many test binaries reported results at all.
///
/// A mutation run is only meaningful if the suite actually ran. Zero result lines means
/// the harness learned nothing, which is different from learning that nothing failed.
fn result_lines(root: &Path) -> usize {
    let Ok(out) = Command::new(cargo())
        .arg("test")
        .arg("--workspace")
        .current_dir(root)
        .output()
    else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("test result:"))
        .count()
}

fn failing_tests(root: &Path) -> usize {
    let Ok(out) = Command::new(cargo())
        .arg("test")
        .arg("--workspace")
        .current_dir(root)
        .output()
    else {
        return 0;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|l| l.strip_prefix("test result: FAILED. "))
        .filter_map(|rest| rest.split(';').nth(1))
        .filter_map(|f| f.trim().strip_suffix(" failed"))
        .filter_map(|n| n.parse::<usize>().ok())
        .sum()
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}
