//! The compiler components written in Nazm, checked against the Rust implementation.
//!
//! # Why this is not a demonstration
//!
//! `compiler/lexer.nz` is the first component of the self-hosted compiler, not a sample
//! program. What makes it a component rather than a demonstration is this file: its
//! token stream is compared against `nazm_syntax::lex`, token for token, span for span,
//! over real sources — including **its own source**. A tokeniser that agreed with
//! nothing would be a program that runs, which is a much weaker claim.
//!
//! # What agreement does and does not establish
//!
//! Differential agreement detects **disagreement**. It cannot detect a bug both
//! implementations share, and since the Nazm one was written from the Rust one that is
//! the likely kind. So the cases below include hand-written expectations for the shapes
//! that are easy to get wrong — two-byte operators against their one-byte prefixes,
//! comments against division, keywords against identifiers that start the same way — and
//! the spans are checked, not just the kinds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/nazm-cli`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .to_path_buf()
}

/// Build `compiler/lexer.nz` once, natively, and return the executable.
fn build_lexer(dir: &Path) -> PathBuf {
    let source = repo_root().join("compiler/lexer.nz");
    assert!(source.exists(), "{} is missing", source.display());
    let exe = dir.join("nzlex");
    let out = Command::new(NAZM)
        .args(["build", source.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "the Nazm lexer no longer compiles natively: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

/// One token as the comparison sees it: kind, start offset, end offset.
type Tok = (i64, u32, u32);

/// `(kind, start, end)` for each token the Nazm lexer produced.
fn nazm_tokens(exe: &Path, file: &Path) -> Vec<Tok> {
    let out = Command::new(exe)
        .arg(file)
        .output()
        .expect("run the Nazm lexer");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the Nazm lexer failed on {}: {}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("the lexer's output is ASCII");
    let mut tokens = Vec::new();
    for line in text.lines() {
        if line == "EOF" {
            break;
        }
        let parts: Vec<&str> = line.split(' ').collect();
        let [kind, start, end] = parts.as_slice() else {
            panic!("unexpected output line {line:?}");
        };
        tokens.push((
            kind.parse().expect("kind"),
            start.parse().expect("start"),
            end.parse().expect("end"),
        ));
    }
    tokens
}

/// The same, from the Rust lexer, with kinds mapped into the Nazm lexer's numbering.
///
/// The mapping lives here, in one place, and is exhaustive on purpose: a token kind
/// added to the language fails to compile this function rather than silently becoming
/// "some other kind" in the comparison.
fn rust_tokens(source: &str) -> Vec<Tok> {
    use nazm_core::__syntax::TokenKind as T;
    let (tokens, errors) = nazm_core::__syntax::lex(source);
    assert!(
        errors.is_empty(),
        "the reference lexer objected: {errors:#?}"
    );
    tokens
        .iter()
        .filter_map(|t| {
            let kind = match &t.kind {
                T::Int(_) | T::IntTooLarge(_) => 1,
                T::Str(_) => 2,
                T::Ident(_) => 3,
                T::Fn => 10,
                T::Let => 11,
                T::If => 12,
                T::Else => 13,
                T::True => 14,
                T::False => 15,
                T::Mut => 16,
                T::While => 17,
                T::Break => 18,
                T::Continue => 19,
                T::Return => 20,
                T::LParen => 30,
                T::RParen => 31,
                T::LBrace => 32,
                T::RBrace => 33,
                T::Comma => 34,
                T::Colon => 35,
                T::Semi => 36,
                T::Arrow => 37,
                T::Eq => 38,
                T::Plus => 40,
                T::Minus => 41,
                T::Star => 42,
                T::Slash => 43,
                T::Percent => 44,
                T::EqEq => 45,
                T::NotEq => 46,
                T::Lt => 47,
                T::LtEq => 48,
                T::Gt => 49,
                T::GtEq => 50,
                // The Rust lexer emits a final `Eof`; the Nazm one prints `EOF` and
                // stops, so there is nothing to compare.
                T::Eof => return None,
            };
            Some((kind, t.span.start, t.span.end))
        })
        .collect()
}

#[test]
fn the_nazm_lexer_agrees_with_the_reference_on_every_source_in_the_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_lexer(dir.path());
    let root = repo_root();

    // Its own source first — the largest and most varied `.nz` file there is, and the
    // one a self-hosting claim will eventually rest on.
    let mut files = vec![root.join("compiler/lexer.nz")];
    for folder in ["examples", "corpus"] {
        let path = root.join(folder);
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "nz") {
                    files.push(p);
                }
            }
        }
    }
    assert!(!files.is_empty(), "no sources to compare against");

    let mut compared = 0usize;
    for file in &files {
        let source = std::fs::read_to_string(file).expect("read");
        let theirs = rust_tokens(&source);
        let ours = nazm_tokens(&exe, file);
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{}: {} tokens from Nazm, {} from the reference",
            file.display(),
            ours.len(),
            theirs.len()
        );
        for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
            assert_eq!(
                a,
                b,
                "{} token {i}: Nazm produced {a:?}, the reference {b:?}",
                file.display()
            );
        }
        compared += ours.len();
    }
    assert!(
        compared > 1500,
        "only {compared} tokens compared — too few to mean much"
    );
}

#[test]
fn the_shapes_that_are_easy_to_get_wrong_are_checked_by_hand() {
    // Differential agreement is satisfied by a shared bug, and the Nazm lexer was
    // written from the Rust one. These expectations are written from `docs/spec.md`.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_lexer(dir.path());

    let cases: &[(&str, &[Tok])] = &[
        // A two-byte operator against its one-byte prefix, both ways round.
        ("a == b", &[(3, 0, 1), (45, 2, 4), (3, 5, 6)]),
        ("a = b", &[(3, 0, 1), (38, 2, 3), (3, 4, 5)]),
        ("a <= b", &[(3, 0, 1), (48, 2, 4), (3, 5, 6)]),
        ("a < b", &[(3, 0, 1), (47, 2, 3), (3, 4, 5)]),
        ("-> -", &[(37, 0, 2), (41, 3, 4)]),
        // A comment against division: the second byte decides.
        ("1 / 2", &[(1, 0, 1), (43, 2, 3), (1, 4, 5)]),
        ("1 // 2", &[(1, 0, 1)]),
        ("1 // c\n2", &[(1, 0, 1), (1, 7, 8)]),
        // A keyword against an identifier that begins with one.
        ("if ifx", &[(12, 0, 2), (3, 3, 6)]),
        ("return returns", &[(20, 0, 6), (3, 7, 14)]),
        ("mutable", &[(3, 0, 7)]),
        // Digit separators are part of the token.
        ("1_000", &[(1, 0, 5)]),
        // A string's span covers its quotes; its text does not.
        ("\"ab\"", &[(2, 0, 4)]),
        ("\"\"", &[(2, 0, 2)]),
        // Multi-byte UTF-8 inside a string stays byte-counted.
        ("\"é\"", &[(2, 0, 4)]),
    ];

    for (source, expected) in cases {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let ours = nazm_tokens(&exe, &file);
        assert_eq!(&ours[..], *expected, "source {source:?}");
        // And the reference agrees with the same hand-written expectation, so neither
        // implementation is being graded against the other alone.
        assert_eq!(&rust_tokens(source)[..], *expected, "reference, {source:?}");
    }
}

#[test]
fn a_lexing_error_is_located_and_stops_the_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_lexer(dir.path());

    for (source, code, offset) in [
        ("fn main() { $ }", "N0001", 12),
        ("let s = \"open", "N0002", 8),
        ("let s = \"open\nlet t = 1;", "N0002", 8),
    ] {
        let file = dir.path().join("bad.nz");
        std::fs::write(&file, source).expect("write");
        let out = Command::new(&exe).arg(&file).output().expect("run");
        assert_eq!(out.status.code(), Some(1), "{source:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(code), "{source:?}: {stderr}");
        assert!(
            stderr.contains(&format!("at {offset}")),
            "{source:?}: expected offset {offset} in {stderr}"
        );
        // The reference reports the same code for the same source, which is what makes
        // the offset above a shared claim rather than this lexer's opinion.
        let (_, errors) = nazm_core::__syntax::lex(source);
        assert!(
            errors.iter().any(|d| d.code.as_str() == code),
            "{source:?}: the reference did not report {code}: {errors:#?}"
        );
    }
}
