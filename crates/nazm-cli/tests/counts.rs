//! The delivery report's test counts add up to the suite's total.
//!
//! A release report breaks the suite into groups. A table of numbers in prose drifts
//! the moment anybody adds a test, and a reader has no way to tell — the numbers look
//! exactly as authoritative either way. This does not check that each row is *right*
//! (that needs running the suite, which is what is running this), but it does check
//! that the rows sum to the stated total, which catches the common failure: a group
//! updated and the total left alone, or the reverse.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Every frozen release report under `docs/internal/releases/`.
///
/// Globbed rather than named: a release report is frozen evidence for one commit, so they
/// accumulate, and a hard-coded path would make this test something to remember to update
/// at exactly the moment nobody is thinking about it.
fn release_reports() -> Vec<PathBuf> {
    let dir = repo_root().join("docs/internal/releases");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no release report in {}", dir.display());
    found
}

#[test]
fn the_delivery_reports_groups_sum_to_its_total() {
    for p in release_reports() {
        check_one(&p);
    }
}

fn check_one(p: &Path) {
    let text = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

    // The one sentence that states the total. The ignored count is read rather than
    // written down here: hard-coding it meant this test had to be edited by hand every
    // time a test was ignored, which is the drift it exists to catch.
    let total: usize = text
        .split("**")
        .find_map(|chunk| {
            let (n, rest) = chunk.split_once(" passed, 0 failed, ")?;
            rest.trim_end().strip_suffix("ignored.")?;
            n.parse().ok()
        })
        .unwrap_or_else(|| {
            panic!(
                "{} should state `**N passed, 0 failed, M ignored.**`",
                p.display()
            )
        });

    // The results table: rows whose second cell is a bare number.
    let rows: Vec<usize> = text
        .lines()
        .filter_map(|line| {
            let mut cells = line.trim().strip_prefix("| ")?.split(" | ");
            cells.next()?;
            cells.next()?.trim().parse().ok()
        })
        .collect();
    assert!(rows.len() >= 8, "only found {} counted rows", rows.len());

    let sum: usize = rows.iter().sum();
    assert_eq!(
        sum, total,
        "the results table sums to {sum} and the total says {total}. One of them was \
         updated and the other was not."
    );
}
