//! The integration tests whose files begin with `a` to `b`, built as one executable.
//!
//! Each file beside this one is a module of one of the `cli_*` executables, by its first
//! letter, so the suite links five executables rather than one per file; `selfhost.rs`
//! stays its own, as only a contained run may start it. A new test file is declared in its
//! executable below — `every_test_file_belongs_to_its_executable` (in `cli_a_b.rs`) fails
//! until it is.

// The helpers below are shared by the modules, which each allow these for themselves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod differential;

mod accel;
mod accel_providers;
mod accounts;
mod apps;
mod args;
mod arrays;
mod atomics;
mod attestation;
mod attributes;
mod authority;
mod boards;
mod borrowed_fields;
mod box_list;
mod build;
mod build_provenance;
mod builtins;
mod bytes_text;

/// Every test file is a module of the executable its first letter names, and of no other.
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn every_test_file_belongs_to_its_executable() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let roots = [
        ("cli_a_b", "ab"),
        ("cli_c_d", "cd"),
        ("cli_e_l", "efghijkl"),
        ("cli_m_r", "mnopqr"),
        ("cli_s_z", "stuvwxyz"),
    ];
    let declared: Vec<(String, String)> = roots
        .iter()
        .map(|(r, _)| {
            let text = std::fs::read_to_string(dir.join(format!("{r}.rs"))).unwrap();
            ((*r).to_owned(), text)
        })
        .collect();
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|_| path.extension().is_some_and(|x| x == "rs"))
        else {
            continue;
        };
        if stem == "selfhost" || roots.iter().any(|(r, _)| *r == stem) {
            continue;
        }
        let first = stem.chars().next().unwrap();
        let line = format!("\nmod {stem};\n");
        for (root, text) in &declared {
            let letters = roots.iter().find(|(r, _)| r == root).unwrap().1;
            assert_eq!(
                text.contains(&line),
                letters.contains(first),
                "`tests/{stem}.rs` and `tests/{root}.rs`"
            );
        }
    }
}
