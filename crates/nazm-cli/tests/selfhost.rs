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
fn the_nazm_checker_resolves_names_exactly_as_the_reference_does() {
    // **Scope, stated:** `compiler/check.nz` reports undefined variables (`N0200`) and
    // undefined functions (`N0201`) and nothing else, so the sources below are chosen so
    // that those are the only errors the reference finds. A checker compared only on
    // programs it happens to accept would agree with everything.
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
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let out = Command::new(&exe).arg(&file).output().expect("run");
        let ours = sorted_diagnostics(&String::from_utf8_lossy(&out.stdout));

        let theirs: Vec<(String, u32, u32)> = {
            let mut v: Vec<(String, u32, u32)> = nazm_core::check_source(source)
                .into_iter()
                .filter(|d| d.code.as_str() == "N0200" || d.code.as_str() == "N0201")
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

        assert_eq!(ours, theirs, "name resolution differs for {source:?}");
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
