//! The delivery report's test counts add up to the suite's total.
//!
//! `docs/delivery.md` breaks 521 tests into groups. A table of numbers in prose drifts
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

#[test]
fn the_delivery_reports_groups_sum_to_its_total() {
    let p = repo_root().join("docs/delivery.md");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));

    // The one sentence that states the total.
    let total: usize = text
        .split("**")
        .find_map(|chunk| chunk.strip_suffix(" passed, 0 failed, 6 ignored."))
        .and_then(|n| n.parse().ok())
        .expect("docs/delivery.md should state `**N passed, 0 failed, 6 ignored.**`");

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
