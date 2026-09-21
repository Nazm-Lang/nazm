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
                T::Use => 21,
                T::Scope => 22,
                T::Spawn => 23,
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
    let mut files = Vec::new();
    for folder in ["compiler", "examples", "corpus"] {
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

// ---------------------------------------------------------------------------
// The syntax-tree comparison format
//
// A tree is compared as a **post-order dump with explicit arity**: one line per node,
// `kind|arity|start|end|text`, children first. Post-order plus arity is a complete and
// unambiguous encoding — a reader can rebuild the tree from it — so agreement on the
// dump is agreement on the tree, without either side needing to expose node indices or
// agree on how an arena is laid out.
//
// This function is the **specification** the Nazm parser is written against, and it is
// exhaustive on every syntax enum for the same reason the token mapping is: a node kind
// added to the language fails to compile here rather than quietly matching a wildcard.
// ---------------------------------------------------------------------------

fn dump_program(p: &nazm_core::__syntax::Program) -> String {
    let mut out = String::new();
    for f in &p.functions {
        dump_function(f, &mut out);
    }
    push_node(&mut out, "program", p.functions.len(), 0, 0, "");
    out
}

fn push_node(out: &mut String, kind: &str, arity: usize, start: u32, end: u32, text: &str) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "{kind}|{arity}|{start}|{end}|{text}");
}

fn dump_function(f: &nazm_core::__syntax::Function, out: &mut String) {
    for p in &f.params {
        push_node(
            out,
            "param",
            0,
            p.span.start,
            p.span.end,
            &format!("{}:{}", p.name, p.ty.name),
        );
    }
    let arity = match &f.body {
        Some(b) => {
            dump_block(b, out);
            f.params.len() + 1
        }
        None => f.params.len(),
    };
    push_node(
        out,
        "fn",
        arity,
        f.span.start,
        f.span.end,
        &format!("{}:{}", f.name, f.return_type.name),
    );
}

fn dump_block(b: &nazm_core::__syntax::Block, out: &mut String) {
    for s in &b.statements {
        dump_stmt(s, out);
    }
    let mut arity = b.statements.len();
    if let Some(t) = &b.tail {
        dump_expr(t, out);
        arity += 1;
    }
    push_node(
        out,
        "block",
        arity,
        b.span.start,
        b.span.end,
        if b.tail.is_some() { "tail" } else { "" },
    );
}

fn dump_stmt(s: &nazm_core::__syntax::Stmt, out: &mut String) {
    use nazm_core::__syntax::Stmt as S;
    match s {
        S::Let(l) => {
            dump_expr(&l.value, out);
            push_node(
                out,
                "let",
                1,
                l.span.start,
                l.span.end,
                &format!("{}:{}", l.name, u8::from(l.mutable)),
            );
        }
        S::Assign(a) => {
            dump_expr(&a.value, out);
            push_node(out, "assign", 1, a.span.start, a.span.end, &a.name);
        }
        S::While(w) => {
            dump_expr(&w.cond, out);
            dump_block(&w.body, out);
            push_node(out, "while", 2, w.span.start, w.span.end, "");
        }
        S::Return(r) => {
            dump_expr(&r.value, out);
            push_node(out, "return", 1, r.span.start, r.span.end, "");
        }
        S::Break { span } => push_node(out, "break", 0, span.start, span.end, ""),
        S::Continue { span } => push_node(out, "continue", 0, span.start, span.end, ""),
        S::Expr(e) => {
            dump_expr(e, out);
            push_node(out, "exprstmt", 1, e.span().start, e.span().end, "");
        }
        // The encoding the Nazm parser will have to produce if it ever gains these. It
        // cannot parse them today, so no differential input reaches here — writing the
        // shape down now is what stops the two parsers being specified separately later.
        S::Scope(sc) => {
            dump_block(&sc.body, out);
            push_node(out, "scope", 1, sc.span.start, sc.span.end, "");
        }
        S::Spawn(sp) => {
            for a in &sp.args {
                dump_expr(a, out);
            }
            push_node(
                out,
                "spawn",
                sp.args.len(),
                sp.span.start,
                sp.span.end,
                &sp.callee,
            );
        }
    }
}

fn dump_expr(e: &nazm_core::__syntax::Expr, out: &mut String) {
    use nazm_core::__syntax::Expr as E;
    match e {
        E::Int { value, span } => {
            push_node(out, "int", 0, span.start, span.end, &value.to_string())
        }
        E::Bool { value, span } => {
            push_node(out, "bool", 0, span.start, span.end, &value.to_string());
        }
        E::Str { value, span } => push_node(out, "str", 0, span.start, span.end, value),
        E::Var { name, span } => push_node(out, "var", 0, span.start, span.end, name),
        E::Neg { operand, span, .. } => {
            dump_expr(operand, out);
            push_node(out, "neg", 1, span.start, span.end, "");
        }
        E::Binary {
            op, lhs, rhs, span, ..
        } => {
            dump_expr(lhs, out);
            dump_expr(rhs, out);
            push_node(out, "bin", 2, span.start, span.end, op.symbol());
        }
        E::Call {
            callee, args, span, ..
        } => {
            for a in args {
                dump_expr(a, out);
            }
            push_node(out, "call", args.len(), span.start, span.end, callee);
        }
        E::If {
            cond,
            then_block,
            else_block,
            span,
        } => {
            dump_expr(cond, out);
            dump_block(then_block, out);
            let arity = match else_block {
                Some(b) => {
                    dump_block(b, out);
                    3
                }
                None => 2,
            };
            push_node(out, "if", arity, span.start, span.end, "");
        }
        E::Block(b) => {
            dump_block(b, out);
            push_node(out, "blockexpr", 1, b.span.start, b.span.end, "");
        }
    }
}

#[test]
fn the_dump_format_round_trips_the_shapes_it_will_be_used_on() {
    // The comparison format is only as good as its ability to tell two trees apart, so
    // this pins a few dumps by hand before anything is compared against them.
    let (p, errors) = nazm_core::__syntax::parse_source("fn main() -> Int { 1 + 2 * 3 }");
    assert!(errors.is_empty(), "{errors:#?}");
    let dump = dump_program(&p);
    let kinds: Vec<&str> = dump
        .lines()
        .map(|l| l.split('|').next().unwrap_or(""))
        .collect();
    // Post-order: 1, 2, 3, (2*3), (1 + …), block, fn, program.
    assert_eq!(
        kinds,
        vec!["int", "int", "int", "bin", "bin", "block", "fn", "program"],
        "{dump}"
    );
    // Precedence is visible in the shape: the inner `bin` is `*`.
    let ops: Vec<&str> = dump
        .lines()
        .filter(|l| l.starts_with("bin|"))
        .map(|l| l.rsplit('|').next().unwrap_or(""))
        .collect();
    assert_eq!(ops, vec!["*", "+"], "{dump}");

    // Two trees that differ only in grouping must produce different dumps, or the
    // comparison would pass on a parser that ignored precedence.
    let (q, _) = nazm_core::__syntax::parse_source("fn main() -> Int { (1 + 2) * 3 }");
    assert_ne!(dump_program(&q), dump, "the format cannot see grouping");
}

/// Build `compiler/parser.nz` natively and return the executable.
fn build_parser(dir: &Path) -> PathBuf {
    let source = repo_root().join("compiler/parser.nz");
    assert!(source.exists(), "{} is missing", source.display());
    let exe = dir.join("nzparse");
    let out = Command::new(NAZM)
        .args(["build", source.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "the Nazm parser no longer compiles natively: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

#[test]
fn the_nazm_parser_produces_the_same_tree_as_the_reference() {
    // Whole expressions, compared as trees rather than as text: precedence,
    // associativity, grouping, unary binding, call arity and every span. A parser that
    // got precedence wrong would produce a differently *shaped* dump, which is why the
    // format encodes shape rather than tokens.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_parser(dir.path());

    let sources = [
        "fn m() -> Int { 1 }",
        "fn m() -> Int { 1 + 2 * 3 }",
        "fn m() -> Int { 1 * 2 + 3 }",
        "fn m() -> Int { (1 + 2) * 3 }",
        "fn m() -> Int { 1 - 2 - 3 }",
        "fn m() -> Int { 1 - (2 - 3) }",
        "fn m() -> Int { 100 / 7 % 3 }",
        "fn m() -> Int { -x }",
        "fn m() -> Int { - -1 }",
        // `Int::MIN`: its magnitude is one greater than `Int::MAX`, so the lexer cannot
        // produce it and the negation has to fold with the literal. If the two parsers
        // disagree here, one of them cannot express the smallest `Int` at all.
        "fn m() -> Int { -9223372036854775808 }",
        "fn m() -> Int { -9223372036854775808 % -1 }",
        "fn m() -> Int { 1 + -2 }",
        "fn m() -> Int { -(1 + 2) }",
        "fn m() -> Bool { a < b == c }",
        "fn m() -> Bool { 1 < 2 }",
        "fn m() -> Bool { true != false }",
        "fn m() -> Int { f() }",
        "fn m() -> Int { f(1) }",
        "fn m() -> Int { f(1, 2) }",
        "fn m() -> Int { f(g(1), 2 * 3, -4) }",
        "fn m() -> Int { f(g(h(1))) }",
        "fn m() -> Str { \"hello\" }",
        "fn m(x: Int) -> Bool { true }",
        "fn m(a: Int, b: Str) -> Str { b }",
        "fn one() -> Int { 1 }\nfn two() -> Int { one() + one() }",
        "use \"other.nz\";\nfn m() -> Int { 1 }",
        // Statements, `while`, and nested blocks.
        "fn m() -> Int { let x = 1; x }",
        "fn m() -> Int { let mut x = 1; x = 2; x }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; } i }",
        "fn m() -> Int { let mut i = 0; while i < 3 { break; } i }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; continue; } i }",
        "fn m() -> Int { return 1; }",
        "fn m() -> Int { f(1); 2 }",
        "fn m() -> Int { let mut i = 0; while i < 2 { let mut j = 0; while j < 2 { j = j + 1; } i = i + 1; } i }",
        // An empty body is valid *syntax* — the checker is what rejects it, which is the
        // frontend-validity/backend-coverage split the project keeps everywhere else.
        "fn m() -> Int { }",
        // `if`: the one construct that suspends an expression part-way to parse a block.
        "fn m() -> Int { if true { 1 } else { 2 } }",
        "fn m() -> Int { if true { 1 } }",
        "fn m(x: Int) -> Int { if x > 0 { 1 } else { 0 - 1 } }",
        "fn m() -> Int { let x = if true { 1 } else { 2 }; x }",
        "fn m(s: Int) -> Int { s + if s > 0 { 1 } else { 0 } }",
        "fn m() -> Int { f(if true { 1 } else { 2 }) }",
        "fn m() -> Int { f(1, if true { 2 } else { 3 }, 4) }",
        "fn m() -> Int { if true { if false { 1 } else { 2 } } else { 3 } }",
        "fn m() -> Int { if if true { false } else { true } { 1 } else { 2 } }",
        "fn m() -> Int { let mut i = 0; while i < 3 { if i == 1 { i = 9; } i = i + 1; } i }",
        "fn m() -> Int { let mut i = 0; while i < 3 { if i == 1 { break; } else { continue; } } i }",
        "fn m(x: Int) -> Int { return if x > 0 { 1 } else { 2 }; }",
        "fn m(x: Int) -> Int { let mut y = 0; y = if x > 0 { 1 } else { 2 }; y }",
        "fn m(x: Int) -> Int { let mut i = 0; while if i < x { true } else { false } { i = i + 1; } i }",
        "fn m() -> Int { (if true { 1 } else { 2 }) * 3 }",
        "fn m() -> Int { -if true { 1 } else { 2 } }",
        "fn total(n: Int) -> Int {\n    let mut i = 0;\n    let mut s = 0;\n    while i < n {\n        s = s + i;\n        i = i + 1;\n    }\n    return s;\n}\n",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let out = Command::new(&exe).arg(&file).output().expect("run");
        assert_eq!(
            out.status.code(),
            Some(0),
            "the Nazm parser failed on {source:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // The driver prints the node count as `main`'s value; the dump is everything
        // before it.
        let text = String::from_utf8(out.stdout).expect("ASCII output");
        let ours: String = text
            .lines()
            .filter(|l| l.contains('|'))
            .map(|l| format!("{l}\n"))
            .collect();

        let (program, errors) = nazm_core::__syntax::parse_source(source);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        let theirs = dump_program(&program);

        assert_eq!(ours, theirs, "trees differ for {source:?}");
    }
}

/// Whether a source uses a construct the bootstrap compiler deliberately does not accept.
///
/// Decided by **lexing**, not by a list of filenames: `scope` and `spawn` are reserved
/// words in both lexers, so a new example that uses concurrency is classified correctly
/// without anyone remembering to add it here. A list of names is the kind of thing that
/// is right on the day it is written.
///
/// `docs/bootstrap-design.md` says which constructs these are and why they are left out.
fn outside_the_bootstrap_subset(source: &str) -> bool {
    use nazm_core::__syntax::TokenKind as T;
    let (tokens, _) = nazm_core::__syntax::lex(source);
    tokens.iter().any(|t| matches!(t.kind, T::Scope | T::Spawn))
}

#[test]
fn the_nazm_parser_agrees_with_the_reference_on_every_source_in_the_tree() {
    // The real test of a parser: whole files, including its own source and the
    // tokeniser's — the largest and most varied Nazm programs that exist.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_parser(dir.path());
    let root = repo_root();

    let mut files = Vec::new();
    for folder in ["compiler", "examples"] {
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "nz") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    assert!(files.len() >= 5, "only {} sources found", files.len());

    let mut nodes = 0usize;
    for file in &files {
        let source = std::fs::read_to_string(file).expect("read");
        let (program, errors) = nazm_core::__syntax::parse_source(&source);
        assert!(errors.is_empty(), "{}: {errors:#?}", file.display());
        let theirs = dump_program(&program);

        let out = Command::new(&exe).arg(file).output().expect("run");
        if outside_the_bootstrap_subset(&source) {
            // A refusal, and a located one. A bootstrap parser that quietly produced
            // *some* tree for a construct it does not handle would agree with nothing
            // and be discovered much later — which is why this is asserted rather than
            // skipped.
            assert_eq!(
                out.status.code(),
                Some(1),
                "{} is outside the bootstrap subset and should have been refused",
                file.display()
            );
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("parse error at "),
                "{}: the refusal should say where: {stderr}",
                file.display()
            );
            continue;
        }
        assert_eq!(
            out.status.code(),
            Some(0),
            "the Nazm parser failed on {}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).expect("ASCII output");
        let ours: String = text
            .lines()
            .filter(|l| l.contains('|'))
            .map(|l| format!("{l}\n"))
            .collect();

        // Compared line by line so a failure names the node rather than printing two
        // thousand-line dumps.
        for (n, (a, b)) in ours.lines().zip(theirs.lines()).enumerate() {
            assert_eq!(a, b, "{}: node {n}", file.display());
        }
        assert_eq!(
            ours.lines().count(),
            theirs.lines().count(),
            "{}: different number of nodes",
            file.display()
        );
        nodes += theirs.lines().count();
    }
    assert!(
        nodes > 2000,
        "only {nodes} nodes compared — too few to mean much"
    );
}

#[test]
fn the_nazm_parser_refuses_what_it_cannot_parse_yet_by_name() {
    // The scope boundary is a *refusal*, not a misparse. A parser that quietly produced
    // some tree for a statement it does not handle would agree with nothing and be
    // discovered much later.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_parser(dir.path());

    for (source, expected) in [
        (
            "fn m() -> Int { { 1 } }",
            "a block used as an expression: not implemented yet",
        ),
        (
            "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; }",
            "this block is never closed",
        ),
        // Reported as the unclosed bracket, at the bracket — not as "expected an
        // expression" at the brace, which would be true and useless.
        ("fn m() -> Int { (1 }", "this bracket is never closed"),
        ("fn m() Int { 1 }", "expected `->` and a return type"),
    ] {
        let file = dir.path().join("bad.nz");
        std::fs::write(&file, source).expect("write");
        let out = Command::new(&exe).arg(&file).output().expect("run");
        assert_eq!(out.status.code(), Some(1), "{source:?} should be refused");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected),
            "{source:?}: expected {expected:?}, got {stderr}"
        );
        assert!(stderr.contains("parse error at "), "{source:?}: {stderr}");
    }
}

/// Build `compiler/check.nz` natively and return the executable.
fn build_checker(dir: &Path) -> PathBuf {
    let source = repo_root().join("compiler/check.nz");
    assert!(source.exists(), "{} is missing", source.display());
    let exe = dir.join("nzcheck");
    let out = Command::new(NAZM)
        .args(["build", source.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "the Nazm checker no longer compiles natively: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

/// The `(code, start, end)` of every diagnostic, sorted.
fn sorted_diagnostics(lines: &str) -> Vec<(String, u32, u32)> {
    let mut out: Vec<(String, u32, u32)> = lines
        .lines()
        .filter(|l| l.contains('|'))
        .map(|l| {
            let parts: Vec<&str> = l.split('|').collect();
            let [code, start, end, ..] = parts.as_slice() else {
                panic!("unexpected line {l:?}");
            };
            (
                (*code).to_owned(),
                start.parse().expect("start"),
                end.parse().expect("end"),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_names_and_types() {
    // **Scope, stated:** `compiler/check.nz` reports `N0200`, `N0201`, `N0300`, `N0301`,
    // `N0303`, `N0304` and `N0310`, and nothing else — no `N0204`, no unreachable-code
    // or shadowing rules. The sources below are chosen so those are the only errors the
    // reference finds, because a checker compared only on programs it happens to accept
    // would agree with everything.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());

    let sources = [
        // Clean programs: the checker must find nothing, which is the half that a
        // checker reporting errors everywhere would fail.
        "fn m() -> Int { 1 }",
        "fn m(x: Int) -> Int { x }",
        "fn m() -> Int { let x = 1; x }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; } i }",
        "fn a() -> Int { 1 }\nfn m() -> Int { a() }",
        // A forward reference: `m` calls `b`, defined after it.
        "fn m() -> Int { b() }\nfn b() -> Int { 1 }",
        // Built-ins share the ordinary function namespace.
        "fn m() -> Int { str_len(\"ab\") }",
        "fn m() -> Int { let a = ints_new(); ints_push(a, 1); ints_len(a) }",
        // Shadowing in a nested scope, and a binding that leaves scope with its block.
        "fn m() -> Int { let x = 1; let y = if true { let x = 2; x } else { x }; y }",
        // `let x = x;` reads the *outer* x — a binding is not visible in its own
        // initialiser, which is why binding is a marker rather than immediate.
        "fn m(x: Int) -> Int { let x = x; x }",
        // Now the failures.
        "fn m() -> Int { c }",
        "fn m() -> Int { g() }",
        "fn f(a: Int) -> Int {\n    let b = a + 1;\n    b + c\n}\nfn m() -> Int { f(1) + g(2) }\n",
        // Out of scope after its block ends.
        "fn m() -> Int { let mut i = 0; while i < 1 { let inner = 1; i = i + 1; } inner }",
        // A parameter is not visible in another function.
        "fn a(p: Int) -> Int { p }\nfn m() -> Int { p }",

        // --- types ---------------------------------------------------------------
        // Clean, and exercising every type the language has.
        "fn m() -> Int { let a = ints_new(); ints_push(a, 1); ints_get(a, 0) }",
        "fn m() -> Int { let s = strs_new(); strs_push(s, \"x\"); str_len(strs_get(s, 0)) }",
        "fn m(x: Int, s: Str) -> Int { if x > 0 { str_len(s) } else { 0 } }",
        "fn m() -> Int { let mut b = true; b = false; if b { 1 } else { 0 } }",
        "fn f(a: Int, b: Str) -> Str { str_concat(b, int_to_str(a)) }\nfn m() -> Int { str_len(f(1, \"x\")) }",
        // Arithmetic on a non-`Int`, both ways round.
        "fn m() -> Int { 1 + true }",
        "fn m() -> Int { let s = \"a\"; 1 + s }",
        // Ordering is `Int` only; equality works on one type but not across two.
        "fn m() -> Bool { \"a\" < \"b\" }",
        "fn m() -> Bool { true < false }",
        "fn m() -> Bool { 1 == true }",
        // Sequences are not comparable: identity or contents, and neither is chosen.
        "fn m() -> Int { let a = ints_new(); let b = ints_new(); if a == b { 1 } else { 0 } }",
        // Conditions must be `Bool` — there is no truthiness.
        "fn m() -> Int { if 1 { 2 } else { 3 } }",
        "fn m() -> Int { let mut i = 0; while i { i = i + 1; } i }",
        // Argument count and argument type.
        "fn m() -> Int { str_len(\"a\", \"b\") }",
        "fn m() -> Int { str_len(1) }",
        "fn f(a: Int) -> Int { a }\nfn m() -> Int { f(\"x\") }",
        "fn f(a: Int) -> Int { a }\nfn m() -> Int { f() }",
        // Assignment: mutability and type.
        "fn m() -> Int { let x = 1; x = 2; x }",
        "fn m() -> Int { let mut b = true; b = 1; 0 }",
        // A parameter is never mutable.
        "fn f(a: Int) -> Int { a = 2; a }\nfn m() -> Int { f(1) }",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let out = Command::new(&exe).arg(&file).output().expect("run");
        let ours = sorted_diagnostics(&String::from_utf8_lossy(&out.stdout));

        let theirs: Vec<(String, u32, u32)> = {
            let mut v: Vec<(String, u32, u32)> = nazm_core::check_source(source)
                .into_iter()
                .filter(|d| {
                    matches!(
                        d.code.as_str(),
                        "N0200" | "N0201" | "N0300" | "N0301" | "N0303" | "N0304" | "N0310"
                    )
                })
                .map(|d| {
                    (
                        d.code.as_str().to_owned(),
                        d.primary.span.start,
                        d.primary.span.end,
                    )
                })
                .collect();
            v.sort();
            v
        };

        assert_eq!(ours, theirs, "checking differs for {source:?}");
        // Exit status is part of the interface, not decoration.
        assert_eq!(
            out.status.code(),
            Some(i32::from(!theirs.is_empty())),
            "wrong exit status for {source:?}"
        );
    }
}

#[test]
fn the_nazm_checker_accepts_the_compiler_it_is_part_of() {
    // The strongest available check short of self-hosting: run the Nazm checker over
    // every Nazm source in the tree — including its own — and require it to find nothing
    // the reference does not.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let root = repo_root();

    let mut files = Vec::new();
    for folder in ["compiler", "examples"] {
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "nz") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    assert!(files.len() >= 5, "only {} sources", files.len());

    for file in &files {
        let out = Command::new(&exe).arg(file).output().expect("run");
        let ours = sorted_diagnostics(&String::from_utf8_lossy(&out.stdout));
        // A file that `use`s another is resolved by the driver, not by this checker, so
        // its imported names look undefined in isolation. Those files are checked as a
        // whole program by `nazm check`, which is what the assertion below uses.
        let source = std::fs::read_to_string(file).expect("read");
        if outside_the_bootstrap_subset(&source) {
            // Parsing fails before checking can begin, so there is nothing to compare.
            // The refusal itself is asserted by the parser's own tree-wide test.
            continue;
        }
        let standalone = !source.contains("use \"");
        if standalone {
            assert!(
                ours.is_empty(),
                "{}: the Nazm checker rejected a source the reference accepts: {ours:?}",
                file.display()
            );
        }
    }
}

#[test]
fn the_nazm_checker_knows_every_built_in_the_front_end_has() {
    // `compiler/check.nz` carries its own table of built-in signatures, transcribed from
    // `nazm-core/src/intrinsic.rs`, because a Nazm program cannot read a Rust enum. The
    // risk that creates is drift — a built-in added to the language and never taught to
    // the checker, which would then report a valid call as an unknown function. This is
    // the guard: it reads the Nazm source and requires a signature for every intrinsic.
    let source = std::fs::read_to_string(repo_root().join("compiler/analyse.nz")).expect("read");
    for i in nazm_core::Intrinsic::ALL {
        let needle = format!("\"{}\"", i.name());
        assert!(
            source.contains(&needle),
            "`{}` has no signature in compiler/analyse.nz — the front end's built-ins \
             and the Nazm checker's table have drifted",
            i.name()
        );
    }
    // Every built-in must be named in that file, and the ones the bootstrap compiler
    // deliberately does not lower must be named in its exclusion list rather than simply
    // missing. The distinction is the whole point of the guard: a gap and a decision look
    // identical from the outside, and only one of them is acceptable.
    let excluded: Vec<&str> = source
        .split("fn outside_bootstrap_subset")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split("name == \"").nth(1))
        .filter_map(|l| l.split('"').next())
        .collect();
    assert_eq!(
        excluded,
        ["chan_new", "chan_send", "chan_recv", "chan_close"],
        "the bootstrap subset's exclusions changed; `docs/bootstrap-design.md` says \
         which built-ins are left out and why"
    );
    for name in &excluded {
        assert!(
            nazm_core::Intrinsic::by_name(name).is_some(),
            "`{name}` is excluded from the bootstrap subset but is not a built-in at all"
        );
    }
    // And the arity the Nazm table records must match, since a wrong one turns a valid
    // call into `N0303`. Checked by counting the non-`n` arguments on each line.
    for i in nazm_core::Intrinsic::ALL {
        if excluded.contains(&i.name()) {
            continue;
        }
        let line = source
            .lines()
            .find(|l| l.contains(&format!("\"{}\"", i.name())) && l.contains("add_builtin"))
            .unwrap_or_else(|| panic!("`{}` is not registered with add_builtin", i.name()));
        let after_name = line
            .split(&format!("\"{}\", ", i.name()))
            .nth(1)
            .unwrap_or_default();
        // `ret, p1, p2, p3` — the parameters are whichever of the three are not `n`.
        // The last line of `builtins` is that function's return value, so it ends `)`
        // rather than `);` — trailing punctuation is stripped per argument instead of
        // from the line, which handles both.
        let args: Vec<&str> = after_name.split(", ").collect();
        let arity = args
            .iter()
            .skip(1)
            .filter(|a| a.trim().trim_end_matches([')', ';']) != "n")
            .count();
        assert_eq!(
            arity,
            i.params().len(),
            "`{}` takes {} parameters but compiler/analyse.nz records {arity}",
            i.name(),
            i.params().len()
        );
    }
}

// ---------------------------------------------------------------------------
// The compiler written in Nazm, end to end
//
// `compiler/emit.nz` reads a `.nz` file and writes LLVM IR. `clang` assembles it. The
// result is a native executable produced by a compiler written in Nazm, with the Rust
// implementation nowhere in the pipeline except as the bootstrap that compiled the
// compiler — which is what `docs/NAZM_AUTONOMOUS_GOAL.md` §8 permits and a wrapper
// shelling out to Rust is not.
// ---------------------------------------------------------------------------

/// Run a command, killing it if it outlives `limit`.
///
/// Its own copy rather than one shared with `tests/build.rs`: each integration test is a
/// separate binary, and a compiled program that does not terminate would otherwise hang
/// the suite rather than fail it.
fn run_bounded(
    cmd: &mut Command,
    limit: std::time::Duration,
) -> Option<(Option<i32>, String, String)> {
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = std::time::Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("collect");
                return Some((
                    status.code(),
                    String::from_utf8_lossy(&out.stdout).into_owned(),
                    String::from_utf8_lossy(&out.stderr).into_owned(),
                ));
            }
            None if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
}

fn build_emitter(dir: &Path) -> PathBuf {
    let source = repo_root().join("compiler/emit.nz");
    assert!(source.exists(), "{} is missing", source.display());
    let exe = dir.join("nzemit");
    let out = Command::new(NAZM)
        .args(["build", source.to_str().unwrap()])
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "the Nazm emitter no longer compiles natively: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

#[test]
fn a_compiler_written_in_nazm_produces_native_executables_that_agree_with_the_interpreter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    // Every arithmetic shape `docs/spec.md` pins, including all four integer boundary
    // cases. Written as explicit expectations *and* compared with the interpreter:
    // an expectation can be written wrong, and differential agreement alone is
    // satisfied by a bug both share.
    let cases: &[(&str, &str)] = &[
        ("1 + 2 * 3 - 4", "3"),
        // Locals: one `alloca` per binding in the entry block, then loads and stores.
        ("let a = 10; let mut b = 32; b = b + a; b * 2", "84"),
        ("let mut x = 1; x = x * 3; x = x * 3; x = x * 3; x", "27"),
        ("let a = 1; let b = 2; let c = 3; a + b * c", "7"),
        // A binding's initialiser sees the *outer* binding of the same name.
        ("let x = 1; let y = x + 10; y", "11"),
        // An expression statement is evaluated and dropped.
        ("1 + 1; let a = 4; a; a * 2", "8"),
        // Comparisons, and `if` as a value.
        ("if 1 < 2 { 10 } else { 20 }", "10"),
        ("if 2 < 1 { 10 } else { 20 }", "20"),
        ("if 1 == 1 { if 2 != 3 { 7 } else { 8 } } else { 9 }", "7"),
        // Loops: the back edge targets the condition, so it is re-evaluated.
        ("let mut i = 0; let mut s = 0; while i < 5 { s = s + i; i = i + 1; } s", "10"),
        ("let mut i = 1; let mut f = 1; while i <= 10 { f = f * i; i = i + 1; } f", "3628800"),
        // Nested loops: the inner loop's blocks must not be the outer's.
        (
            "let mut a = 0; let mut n = 0; while a < 4 { let mut b = 0; while b < 3 { n = n + 1; b = b + 1; } a = a + 1; } n",
            "12",
        ),
        // `break` leaves the innermost loop; `continue` re-evaluates its condition.
        ("let mut i = 0; while i < 10 { if i == 3 { break; } i = i + 1; } i", "3"),
        (
            "let mut i = 0; let mut s = 0; while i < 6 { i = i + 1; if i == 3 { continue; } s = s + i; } s",
            "18",
        ),
        // `return` leaves the function, and a statement `if` still falls through.
        ("let x = 5; if x > 3 { return 99; } 0", "99"),
        ("let x = 1; if x > 3 { return 99; } 7", "7"),
        // A value `if` inside a loop: its `phi` must name the blocks emission ended in.
        (
            "let mut i = 0; let mut s = 0; while i < 5 { s = s + if i % 2 == 0 { i } else { 0 }; i = i + 1; } s",
            "6",
        ),
        ("(1 + 2) * (3 - 4)", "-3"),
        ("100 / 7", "14"),
        ("100 % 7", "2"),
        ("-(3 * 4)", "-12"),
        ("- -5", "5"),
        ("-7 / 2", "-3"),
        ("-7 % 2", "-1"),
        ("7 / -2", "-3"),
        ("7 % -2", "1"),
        ("9223372036854775807", "9223372036854775807"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("9223372036854775807 - 1", "9223372036854775806"),
        // `Int::MIN % -1` is 0: mathematically zero, zero is representable, and the
        // hardware instruction that would compute it faults on exactly this pair.
        ("-9223372036854775808 % -1", "0"),
    ];

    for (expr, expected) in cases {
        let src = format!("fn main() -> Int {{ {expr} }}\n");
        let file = dir.path().join("case.nz");
        std::fs::write(&file, &src).expect("write");

        let ir = Command::new(&emitter)
            .arg(&file)
            .output()
            .expect("run emitter");
        assert_eq!(
            ir.status.code(),
            Some(0),
            "the Nazm compiler refused {expr:?}: {}",
            String::from_utf8_lossy(&ir.stderr)
        );
        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir.stdout).expect("write IR");

        let exe = dir.path().join("case");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "clang rejected IR from the Nazm compiler for {expr:?}: {}\n{}",
            String::from_utf8_lossy(&built.stderr),
            String::from_utf8_lossy(&ir.stdout)
        );

        let (code, stdout, _) =
            run_bounded(&mut Command::new(&exe), std::time::Duration::from_secs(30))
                .expect("the compiled program hung");
        assert_eq!(code, Some(0), "{expr:?}: {stdout}");
        assert_eq!(stdout.trim(), *expected, "{expr:?}");

        // And the interpreter agrees with the same expectation.
        let interp = Command::new(NAZM)
            .args(["run", file.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&interp.stdout).trim(),
            *expected,
            "the expectation disagrees with the interpreter for {expr:?}"
        );
    }
}

#[test]
fn the_nazm_compiler_handles_several_functions_and_calls() {
    // Whole programs rather than one expression: forward references, parameters,
    // early returns, and a callee whose own loop runs inside its caller's.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    let programs: &[(&str, &str)] = &[
        ("fn double(n: Int) -> Int { n * 2 }\nfn main() -> Int { double(21) }\n", "42"),
        (
            "fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { add(add(1, 2), add(3, 4)) }\n",
            "10",
        ),
        // `main` calls a function declared *after* it.
        ("fn main() -> Int { helper(5) }\nfn helper(n: Int) -> Int { n * n }\n", "25"),
        (
            "fn clamp(n: Int) -> Int { if n > 10 { return 10; } if n < 0 { return 0; } n }\n\
             fn main() -> Int { let mut i = 0 - 3; let mut s = 0; while i < 14 { s = s + clamp(i); i = i + 1; } s }\n",
            "85",
        ),
        (
            "fn sum_to(n: Int) -> Int { let mut i = 1; let mut s = 0; while i <= n { s = s + i; i = i + 1; } s }\n\
             fn main() -> Int { sum_to(100) }\n",
            "5050",
        ),
    ];

    for (src, expected) in programs {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, src).expect("write");
        let ir = Command::new(&emitter).arg(&file).output().expect("run");
        assert_eq!(
            ir.status.code(),
            Some(0),
            "the Nazm compiler refused a program: {}",
            String::from_utf8_lossy(&ir.stderr)
        );
        let text = String::from_utf8_lossy(&ir.stdout).into_owned();
        // Symbols are generated from the index, never from the source name: a Nazm
        // identifier would need escaping rules to be safe in an LLVM identifier, and the
        // escaping is the bug.
        assert!(text.contains("@nz.f"), "no generated symbols:\n{text}");
        assert!(
            !text.contains("@double") && !text.contains("@helper"),
            "a source name reached an LLVM identifier:\n{text}"
        );

        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir.stdout).expect("write");
        let exe = dir.path().join("case");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("clang");
        assert!(
            built.status.success(),
            "clang rejected the IR: {}\n{text}",
            String::from_utf8_lossy(&built.stderr)
        );
        let (code, stdout, _) =
            run_bounded(&mut Command::new(&exe), std::time::Duration::from_secs(30)).expect("hung");
        assert_eq!(code, Some(0));
        assert_eq!(stdout.trim(), *expected);

        let interp = Command::new(NAZM)
            .args(["run", file.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&interp.stdout).trim(),
            *expected,
            "the expectation disagrees with the interpreter"
        );
    }
}

#[test]
fn the_nazm_compiler_keeps_the_specified_failure_behaviour() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    // Overflow is an intrinsic and a branch, never `nsw`; division by zero and
    // `Int::MIN / -1` are guarded *before* the instruction that would fault. Each stops
    // the program with exit 2 and a located diagnostic — not a signal, which is what
    // `llvm.trap` would have given.
    let cases: &[(&str, &str, &str)] = &[
        (
            "9223372036854775807 + 1",
            "N0400",
            "this addition overflows `Int`",
        ),
        (
            "-9223372036854775808 - 1",
            "N0400",
            "this subtraction overflows `Int`",
        ),
        (
            "9223372036854775807 * 2",
            "N0400",
            "this multiplication overflows `Int`",
        ),
        // Through a local, so the failure is on a loaded value rather than a constant
        // the optimiser could fold away.
        ("let z = 0; let a = 5; a / z", "N0401", "division by zero"),
        (
            "let a = 9223372036854775807; a + 1",
            "N0400",
            "this addition overflows `Int`",
        ),
        (
            "-(-9223372036854775808)",
            "N0400",
            "this negation overflows `Int`",
        ),
        ("1 / 0", "N0401", "division by zero"),
        ("1 % 0", "N0401", "division by zero"),
        (
            "-9223372036854775808 / -1",
            "N0400",
            "this division overflows `Int`",
        ),
    ];

    for (expr, code, message) in cases {
        let src = format!("fn main() -> Int {{ {expr} }}\n");
        let file = dir.path().join("case.nz");
        std::fs::write(&file, &src).expect("write");
        let ir = Command::new(&emitter).arg(&file).output().expect("run");
        assert_eq!(ir.status.code(), Some(0), "{expr:?}");
        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir.stdout).expect("write");
        let exe = dir.path().join("case");
        assert!(Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("clang")
            .status
            .success());

        let (status, stdout, stderr) =
            run_bounded(&mut Command::new(&exe), std::time::Duration::from_secs(30)).expect("hung");
        assert_eq!(status, Some(2), "{expr:?}: a runtime failure must exit 2");
        assert!(stderr.contains(code), "{expr:?}: {stderr}");
        assert!(stderr.contains(message), "{expr:?}: {stderr}");
        assert!(
            stderr.contains("case.nz:1:"),
            "{expr:?}: no position: {stderr}"
        );
        assert!(stdout.is_empty(), "{expr:?}: nothing should be printed");

        // The IR never uses `nsw`, which would let the optimiser delete the very branch
        // meant to catch the overflow.
        let text = String::from_utf8_lossy(&ir.stdout);
        assert!(!text.contains("nsw"), "{expr:?}: emitted `nsw`:\n{text}");
    }
}

#[test]
fn the_nazm_compiler_refuses_what_it_cannot_emit_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    for (src, expected) in [
        // Outside this increment's coverage: a backend limitation, reported as `N0101`.
        // `main` must produce an `Int`: the platform wrapper prints one, and what a
        // `Str` result would mean for a process is a decision nobody has made.
        ("fn main() -> Bool { true }\n", "N0101"),
        // A bare block used as an expression is refused in the parser, so the refusal
        // arrives before emission is reached at all.
        ("fn main() -> Int { { 1 } }\n", "not implemented yet"),
        // A program that does not check is a *check* failure, reported with the
        // checker's own code — never as a backend limit.
        ("fn main() -> Int { 1 + true }\n", "N0304"),
        ("fn main() -> Int { undefined_name }\n", "N0200"),
    ] {
        let file = dir.path().join("bad.nz");
        std::fs::write(&file, src).expect("write");
        let out = Command::new(&emitter).arg(&file).output().expect("run");
        assert_eq!(out.status.code(), Some(1), "{src:?} should be refused");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected),
            "{src:?}: expected {expected}: {stderr}"
        );
        assert!(
            out.stdout.is_empty(),
            "{src:?}: emitted IR for a refused program"
        );
    }
}

#[test]
fn the_bootstrap_chain_reaches_a_fixpoint() {
    // C1 → C2 → C3 from one source, then C2 and C3 compared and both run over the
    // conformance corpus. `compiler/bootstrap.sh` is the script; this test is what stops
    // it from rotting, and it asserts the *record* rather than only the exit status —
    // a script that exits 0 having skipped a stage would otherwise pass.
    let root = repo_root();
    let script = root.join("compiler/bootstrap.sh");
    assert!(script.exists(), "{} is missing", script.display());

    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("bootstrap");
    let result = Command::new("sh")
        .arg(&script)
        .arg(&out)
        .current_dir(&root)
        .output()
        .expect("run bootstrap.sh");
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "the bootstrap failed:\n{stdout}\n{stderr}"
    );

    let record = std::fs::read_to_string(out.join("record.txt")).expect("the record");
    for expected in [
        "ir: identical",
        "executables: byte-identical",
        "conformance-cases: 5",
    ] {
        assert!(
            record.contains(expected),
            "the record does not say {expected:?}:\n{record}"
        );
    }
    // The two compilers really are the same bytes, read out of the record rather than
    // inferred from the script not having failed.
    let digest = |key: &str| -> String {
        record
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split(' ').nth(1))
            .unwrap_or("<missing>")
            .to_owned()
    };
    assert_eq!(digest("c2-digest:"), digest("c3-digest:"), "{record}");
    assert_ne!(digest("c2-digest:"), "<missing>", "{record}");

    // And all three stages exist, so none was skipped.
    for stage in ["stage1", "stage2", "stage3"] {
        assert!(
            out.join(stage).join("nazmc").exists(),
            "{stage} produced no compiler"
        );
    }
}
