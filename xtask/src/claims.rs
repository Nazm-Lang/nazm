//! The performance claims register (N92, `docs/internal/architecture.md` §7.93).
//!
//! `bench/claims.toml` files every measured section of `docs/internal/performance-log.md` — every `##` or `###`
//! heading that says *measured* or *frozen* — as **reproducible** (a command the tree contains
//! regenerates its numbers), **dated** (a record of the tree on that date, not a claim about this
//! one), or **unreproduced** (a current claim no command regenerates). This gate keeps the register
//! and the document in step and every reproducible entry's command real. It does not run them:
//! whether the numbers still hold is the benchmark's question, asked contained.

use std::collections::BTreeMap;
use std::path::Path;

const STATUSES: &[&str] = &["reproducible", "dated", "unreproduced"];

/// The measured headings of `performance.md`, without their `#`s.
fn measured(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with("## ") || l.starts_with("### "))
        .map(|l| l.trim_start_matches('#').trim().to_owned())
        .filter(|h| h.contains("measured") || h.contains("frozen"))
        .collect()
}

/// Whether a test function of this name is in the tree's Rust sources.
fn test_exists(root: &Path, name: &str) -> bool {
    // A test in a module of an executable that gathers several is named by its path.
    let last = name.rsplit("::").next().unwrap_or(name);
    let needle = format!("fn {last}(");
    let mut stack = vec![root.join("crates"), root.join("xtask")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs")
                && std::fs::read_to_string(&p).is_ok_and(|t| t.contains(&needle))
            {
                return true;
            }
        }
    }
    false
}

/// What is wrong with a reproducible entry's command, if anything: a test it names that the tree
/// does not contain, or an `xtask` command that does not exist.
fn command_problem(root: &Path, how: &str) -> Option<String> {
    let words: Vec<&str> = how.split_whitespace().collect();
    if let Some(at) = words.iter().position(|w| *w == "--ignored") {
        let name = words
            .iter()
            .skip(at + 1)
            .rfind(|w| !w.starts_with('-'))
            .copied()?;
        return (!test_exists(root, name)).then(|| format!("no test `{name}` in the tree"));
    }
    if let ["cargo", "xtask", sub, ..] = words[..] {
        let main = std::fs::read_to_string(root.join("xtask/src/main.rs")).unwrap_or_default();
        return (!main.contains(&format!("Some(\"{sub}\")")))
            .then(|| format!("no `cargo xtask {sub}`"));
    }
    Some(format!(
        "`{how}` is neither a measurement test run with `--ignored` nor a `cargo xtask` command"
    ))
}

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let doc =
        std::fs::read_to_string(root.join("docs/internal/performance-log.md")).map_err(|e| {
            vec![format!(
                "could not read docs/internal/performance-log.md: {e}"
            )]
        })?;
    let register = std::fs::read_to_string(root.join("bench/claims.toml"))
        .map_err(|e| vec![format!("could not read bench/claims.toml: {e}")])?;
    let value: toml::Value = toml::from_str(&register)
        .map_err(|e| vec![format!("bench/claims.toml is not TOML: {e}")])?;
    let claims = value
        .get("claim")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut problems = Vec::new();
    let mut filed: BTreeMap<String, usize> = BTreeMap::new();
    for c in &claims {
        let field = |k: &str| c.get(k).and_then(toml::Value::as_str).unwrap_or("");
        let heading = field("heading");
        *filed.entry(heading.to_owned()).or_default() += 1;
        let status = field("status");
        if !STATUSES.contains(&status) {
            problems.push(format!(
                "`{heading}`: status `{status}` is not one of {STATUSES:?}"
            ));
            continue;
        }
        match status {
            "reproducible" => {
                if field("metric").is_empty() {
                    problems.push(format!("`{heading}`: reproducible, with no `metric`"));
                }
                if let Some(p) = command_problem(root, field("how")) {
                    problems.push(format!("`{heading}`: {p}"));
                }
            }
            _ if field("why").is_empty() => {
                problems.push(format!("`{heading}`: {status}, with no `why`"));
            }
            _ => {}
        }
    }
    let headings = measured(&doc);
    for h in &headings {
        match filed.get(h) {
            None => problems.push(format!(
                "docs/internal/performance-log.md measures `{h}`, and bench/claims.toml does not file it"
            )),
            Some(n) if *n > 1 => problems.push(format!("`{h}` is filed {n} times")),
            Some(_) => {}
        }
    }
    for h in filed.keys() {
        if !headings.contains(h) {
            problems.push(format!(
                "bench/claims.toml files `{h}`, which docs/internal/performance-log.md does not measure"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_measured_heading_is_one_that_says_so() {
        let doc = "# Title\n## Rules\n### Packages — N46, measured 2026-10-01\n### Notes\n\
                   ## The v1 baseline — N48, frozen 2026-10-01\n";
        assert_eq!(
            measured(doc),
            [
                "Packages — N46, measured 2026-10-01",
                "The v1 baseline — N48, frozen 2026-10-01"
            ]
        );
    }

    #[test]
    fn a_command_must_be_a_measurement_test_or_an_xtask_command() {
        let root = crate::guide::root();
        assert_eq!(
            command_problem(
                &root,
                "cargo test -p nazm-cli --test cli_m_r -- --ignored packages::what_packages_cost"
            ),
            None
        );
        assert!(
            command_problem(&root, "cargo test -- --ignored no_such_measurement_here")
                .is_some_and(|p| p.contains("no test"))
        );
        assert_eq!(command_problem(&root, "cargo xtask contained bench"), None);
        assert!(command_problem(&root, "cargo xtask nothing").is_some());
        assert!(command_problem(&root, "python3 measure.py").is_some());
    }

    /// A root holding only a performance document and a register.
    fn fixture(doc: &str, register: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nazm-claims-{}-{}",
            std::process::id(),
            doc.len() * 31 + register.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/internal")).unwrap_or(());
        std::fs::create_dir_all(dir.join("bench")).unwrap_or(());
        std::fs::write(dir.join("docs/internal/performance-log.md"), doc).unwrap_or(());
        std::fs::write(dir.join("bench/claims.toml"), register).unwrap_or(());
        dir
    }

    #[test]
    fn the_register_and_the_document_are_held_together() {
        let doc = "### Packages — N46, measured 2026-10-01\n### Old — measured 2026-09-01\n";
        let good = "[[claim]]\nheading = \"Packages — N46, measured 2026-10-01\"\nstatus = \"dated\"\nwhy = \"history\"\n\n\
                    [[claim]]\nheading = \"Old — measured 2026-09-01\"\nstatus = \"unreproduced\"\nwhy = \"no command\"\n";
        let root = fixture(doc, good);
        assert_eq!(check(&root), Ok(()));
        // A measured heading the register does not file.
        let missing = fixture(&format!("{doc}### New — N93, measured 2026-10-05\n"), good);
        let e = check(&missing).unwrap_err();
        assert!(e.iter().any(|p| p.contains("does not file it")), "{e:?}");
        // A reproducible entry whose test the tree does not hold — the fixture holds no tree.
        let invented = good.replace(
            "status = \"dated\"\nwhy = \"history\"",
            "status = \"reproducible\"\nhow = \"cargo test -- --ignored what_nothing_costs\"\nmetric = \"ms\"",
        );
        let invented = fixture(doc, &invented);
        let e = check(&invented).unwrap_err();
        assert!(
            e.iter().any(|p| p.contains("no test `what_nothing_costs`")),
            "{e:?}"
        );
        // A dated entry must say why, and an unknown status is refused.
        let bare = fixture(doc, &good.replace("why = \"history\"\n", ""));
        assert!(check(&bare).is_err());
        let unknown = fixture(
            doc,
            &good.replace("status = \"dated\"", "status = \"probably\""),
        );
        assert!(check(&unknown).is_err());
        for d in [root, missing, invented, bare, unknown] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
