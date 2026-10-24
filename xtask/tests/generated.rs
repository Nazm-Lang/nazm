//! The files the Nazm toolchain carries instead of reading the checkout (R8 slice 12) are what
//! their generators print from the checkout today: a change to `library/` or to the documentation
//! corpus that is not regenerated would ship the Nazm-built `nazm` with stale text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Runs `generator` on the checkout and compares its output with the committed `file`.
fn fresh(generator: &str, file: &str) {
    let python = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !python {
        eprintln!("skipped: python3 is not on PATH, so {generator} cannot be run");
        return;
    }
    let root = root();
    let out = Command::new("python3")
        .arg("-I")
        .arg(root.join(generator))
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{generator}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let committed = std::fs::read(root.join(file)).unwrap();
    assert!(
        out.stdout == committed,
        "{file} is not what {generator} prints from the checkout: regenerate it \
         (python3 {generator} . > {file})"
    );
}

#[test]
fn the_carried_standard_library_is_the_checkouts() {
    fresh(
        "toolchain/commands/tests/gen-stdlib.py",
        "compiler/stdlib_data.nz",
    );
}

#[test]
fn the_carried_documentation_corpus_is_the_checkouts() {
    fresh(
        "toolchain/commands/tests/gen-docsdata.py",
        "toolchain/commands/docsdata.nz",
    );
}
