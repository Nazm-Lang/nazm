//! R1.5 3.4 (design D13): whole-program optimisation (`docs/architecture.md` §7.120).
//!
//! What a whole build shows: an executable the LLVM backend builds at `-O2` on macOS is linked
//! from LLVM bitcode — the program's units, the standard library's and the runtime's, every one
//! filed as bitcode — and prints what the same program built at `-O0`, with Cranelift and with
//! `--debug` prints, none of which is bitcode; reused from the store, it is the same program.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential::nazm;

use std::path::Path;

/// Strings, a map and the standard library's hash: units in three modules and the runtime.
const WORDS: &str = "fn main() -> Int {
    let counts = hashmap_new[Str, Int]();
    let mut i = 0;
    while i < 2000 {
        let key = int_to_str(i % 37);
        counts[key] += i;
        i = i + 1;
    }
    print(f\"{counts.len()} {counts.top(1)[0].1}\\n\");
    0
}
";

/// How many of the object store's entries hold LLVM bitcode, and how many there are.
fn bitcode(dir: &Path) -> (usize, usize) {
    let mut all = 0;
    let mut bc = 0;
    let store = dir.join(".nazm").join("native");
    for shard in std::fs::read_dir(&store).into_iter().flatten().flatten() {
        for entry in std::fs::read_dir(shard.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let bytes = std::fs::read(entry.path()).unwrap();
            all += 1;
            let has = |m: &[u8]| bytes.windows(4).any(|w| w == m);
            if has(b"BC\xc0\xde") || has(b"\xde\xc0\x17\x0b") {
                bc += 1;
            }
        }
    }
    (bc, all)
}

/// `WORDS` built with `args` in a fresh project: its output and its store's bitcode count.
fn built(args: &[&str]) -> (String, (usize, usize)) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), WORDS).unwrap();
    let mut all = vec!["build", "prog.nz", "-o", "prog"];
    all.extend(args);
    let b = nazm(d.path(), &all);
    assert_eq!(b.code, Some(0), "{args:?}: {}", b.stderr);
    let out = std::process::Command::new(d.path().join("prog"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}");
    // Built again from the store: the same program.
    let again = nazm(d.path(), &all);
    assert_eq!(again.code, Some(0), "{args:?}: {}", again.stderr);
    let out2 = std::process::Command::new(d.path().join("prog"))
        .output()
        .unwrap();
    assert_eq!(out.stdout, out2.stdout, "{args:?}");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        bitcode(d.path()),
    )
}

#[test]
fn an_optimised_macos_build_is_linked_from_bitcode_and_prints_what_the_others_print() {
    let (want, (bc, all)) = built(&["--opt-level", "0"]);
    assert_eq!(want, "37 55000\n0\n");
    assert!(all > 0);
    assert_eq!(bc, 0, "-O0 filed bitcode");
    let (got, (bc, all)) = built(&["--opt-level", "2"]);
    assert_eq!(got, want);
    if cfg!(target_os = "macos") {
        assert_eq!(
            bc, all,
            "an -O2 build filed {bc} of {all} objects as bitcode"
        );
    } else {
        assert_eq!(bc, 0, "whole-program optimisation is macOS's alone");
    }
    for args in [
        &["--opt-level", "2", "--backend", "cranelift"][..],
        &["--opt-level", "2", "--debug"][..],
    ] {
        let (got, (bc, _)) = built(args);
        assert_eq!(got, want, "{args:?}");
        assert_eq!(bc, 0, "{args:?} filed bitcode");
    }
}
