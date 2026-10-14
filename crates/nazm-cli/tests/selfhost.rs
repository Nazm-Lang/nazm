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

mod common;

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
    common::require_containment("lexer.nz");
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
                // Gate 2's numbers and bit operators, outside the Nazm-written compiler's
                // subset; no source this test reads writes one.
                T::Float(_) => 60,
                T::Amp => 61,
                T::Pipe => 62,
                T::Caret => 63,
                T::Tilde => 64,
                T::Shl => 65,
                T::Shr => 66,
                // `@` (Gate 2), which the Nazm-written lexer refuses as an unknown byte; no source
                // this test reads writes one.
                T::At => 67,
                // `..` (R1a), between a `for` range's bounds — outside the Nazm-written
                // compiler's subset until R2; no source this test reads writes one.
                T::DotDot => 68,
                // Compound assignment (R1, 5b.12), outside the Nazm-written compiler's subset
                // until R2; no source this test reads writes one.
                T::PlusEq
                | T::MinusEq
                | T::StarEq
                | T::SlashEq
                | T::PercentEq
                | T::AmpEq
                | T::PipeEq
                | T::CaretEq
                | T::ShlEq
                | T::ShrEq => 70,
                T::Str(_) => 2,
                // An interpolated string (L4), outside the Nazm-written compiler's subset until R2.
                T::FStr(_) => 69,
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
                T::Pub => 24,
                T::Struct => 25,
                T::Enum => 26,
                T::Match => 27,
                T::LParen => 30,
                T::RParen => 31,
                T::LBrace => 32,
                T::RBrace => 33,
                T::Comma => 34,
                T::Colon => 35,
                T::Semi => 36,
                T::Arrow => 37,
                T::Eq => 38,
                T::Dot => 39,
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
                T::FatArrow => 51,
                T::LBracket => 52,
                T::RBracket => 53,
                T::Question => 54,
                // An effect set's `!` (N36; the Nazm-written lexer's since N102).
                T::Bang => 55,
                // `&&` and `||` (N49), which the Nazm-written lexer refuses by name; no source
                // this test reads writes one.
                T::AmpAmp => 56,
                T::PipePipe => 57,
                // `::` (N49), which the Nazm-written lexer refuses by name.
                T::ColonColon => 58,
                // `trait` and `impl` (N78; keywords to the Nazm-written lexer since N102).
                T::Trait => 28,
                T::Impl => 29,
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
        let (status, _, stderr) = run_stage(&exe, &file);
        assert_eq!(status, Some(1), "{source:?}");
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
    // **Source order**, across both lists. The Nazm parser appends to one arena as it
    // reads, so its items come out interleaved; the reference keeps functions and records
    // apart and has to be sorted back. Sorting by where each item was written is what both
    // parsers actually see, so it is the honest reconstruction rather than a convention
    // the two agreed on.
    enum Item<'a> {
        Fn(&'a nazm_core::__syntax::Function),
        Record(&'a nazm_core::__syntax::Record),
        Enum(&'a nazm_core::__syntax::Enum),
    }
    let mut items: Vec<(u32, Item<'_>)> = Vec::new();
    for f in &p.functions {
        items.push((f.span.start, Item::Fn(f)));
    }
    for r in &p.records {
        items.push((r.span.start, Item::Record(r)));
    }
    for e in &p.enums {
        items.push((e.span.start, Item::Enum(e)));
    }
    items.sort_by_key(|(at, _)| *at);
    for (_, item) in &items {
        match item {
            Item::Fn(f) => dump_function(f, &mut out),
            Item::Record(r) => dump_record(r, &mut out),
            Item::Enum(e) => dump_enum(e, &mut out),
        }
    }
    push_node(&mut out, "program", items.len(), 0, 0, "");
    out
}

/// A written type as the Nazm parser's node text carries it (`compiler/parse.nz`, *types,
/// as the text a node carries*): its name alone when it has no arguments, and otherwise
/// every name with its offset and every `]` with the offset just past it.
fn type_text(t: &nazm_core::__syntax::TypeName) -> String {
    if t.args.is_none() {
        return t.name.clone();
    }
    positional(t)
}

fn positional(t: &nazm_core::__syntax::TypeName) -> String {
    let mut s = format!("{}@{}", t.name, t.name_span.start);
    if let Some(args) = &t.args {
        let inner: Vec<String> = args.iter().map(positional).collect();
        s.push_str(&format!("[{}]@{}", inner.join(","), t.span.end));
    }
    s
}

/// Type arguments written after a name in an expression: `[Int@9]`, without the list's
/// own closing offset — a diagnostic about them points at the name they follow.
fn type_args_text(args: Option<&Vec<nazm_core::__syntax::TypeName>>) -> String {
    match args {
        None => String::new(),
        Some(a) => format!(
            "[{}]",
            a.iter().map(positional).collect::<Vec<_>>().join(",")
        ),
    }
}

/// Declared type parameters: `[A@7,B@10]`, or nothing.
fn type_params_text(params: &[nazm_core::__syntax::TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = params
        .iter()
        .map(|p| match &p.requires {
            // N14: a requirement rides after the parameter's position, as `T@5=Equality@8`.
            Some(r) => format!("{}@{}={}@{}", p.name, p.span.start, r.name, r.span.start),
            None => format!("{}@{}", p.name, p.span.start),
        })
        .collect();
    format!("[{}]", inner.join(","))
}

fn dump_record(r: &nazm_core::__syntax::Record, out: &mut String) {
    for f in &r.fields {
        push_node(
            out,
            "structfield",
            0,
            f.span.start,
            f.span.end,
            &format!("{}:{}", f.name, type_text(&f.ty)),
        );
    }
    push_node(
        out,
        "record",
        r.fields.len(),
        r.span.start,
        r.span.end,
        &format!("{}{}", r.name, type_params_text(&r.type_params)),
    );
}

fn dump_enum(e: &nazm_core::__syntax::Enum, out: &mut String) {
    for v in &e.variants {
        for f in &v.fields {
            push_node(
                out,
                "payload",
                0,
                f.span.start,
                f.span.end,
                &format!("{}:{}", f.name, type_text(&f.ty)),
            );
        }
        push_node(
            out,
            "variant",
            v.fields.len(),
            v.span.start,
            v.span.end,
            &v.name,
        );
    }
    push_node(
        out,
        "enum",
        e.variants.len(),
        e.span.start,
        e.span.end,
        &format!("{}{}", e.name, type_params_text(&e.type_params)),
    );
}

fn push_node(out: &mut String, kind: &str, arity: usize, start: u32, end: u32, text: &str) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "{kind}|{arity}|{start}|{end}|{text}");
}

fn dump_function(f: &nazm_core::__syntax::Function, out: &mut String) {
    // What was written: the `out: OutCap` a `main` is given (R1, 5b.1) has no width, and the
    // compiler written in Nazm, which does not give it yet, reads no such parameter.
    let params: Vec<_> = f
        .params
        .iter()
        .filter(|p| p.span.start != p.span.end)
        .collect();
    for p in &params {
        push_node(
            out,
            "param",
            0,
            p.span.start,
            p.span.end,
            &format!("{}:{}", p.name, type_text(&p.ty)),
        );
    }
    let arity = match &f.body {
        Some(b) => {
            dump_block(b, out);
            params.len() + 1
        }
        None => params.len(),
    };
    push_node(
        out,
        "fn",
        arity,
        f.span.start,
        f.span.end,
        // A return type always carries its offset: the `fn` node's span is the whole
        // function, so nothing else says where the type is written.
        &format!(
            "{}{}:{}",
            f.name,
            type_params_text(&f.type_params),
            positional(&f.return_type)
        ),
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
            // The place, as the binding's name and the field path it walks — the same
            // text both parsers have to produce, so a path written to one field depth in
            // one parser and another in the other is a difference the dump shows.
            let mut place = a.place.name.clone();
            for f in &a.place.path {
                match f {
                    nazm_core::__syntax::PlaceStep::Field(f) => {
                        place.push('.');
                        place.push_str(&f.name);
                    }
                    // An index (Gate 2), outside the Nazm-written compiler's subset.
                    nazm_core::__syntax::PlaceStep::Index { .. } => place.push_str("[]"),
                }
            }
            push_node(out, "assign", 1, a.span.start, a.span.end, &place);
        }
        S::While(w) => {
            dump_expr(&w.cond, out);
            dump_block(&w.body, out);
            push_node(out, "while", 2, w.span.start, w.span.end, "");
        }
        // A `for` loop (R1a), outside the Nazm-written compiler's subset until R2: no source
        // this test reads writes one, so the shape here is only the Rust parser's.
        S::For(f) => {
            for e in f.iter.exprs() {
                dump_expr(e, out);
            }
            dump_block(&f.body, out);
            let arity = f.iter.exprs().len() + 1;
            push_node(out, "for", arity, f.span.start, f.span.end, &f.name);
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
        // R1b: the Nazm parser does not carry `defer` until R2, so no differential input has one.
        S::Defer(df) => {
            dump_block(&df.body, out);
            push_node(out, "defer", 1, df.span.start, df.span.end, "");
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
        // Gate 2's arrays, outside the Nazm-written compiler's subset: no agreement corpus holds
        // one.
        E::Array { span, .. } | E::ArrayRepeat { span, .. } | E::Index { span, .. } => {
            push_node(out, "array", 0, span.start, span.end, "");
        }
        // A tuple (L5), outside the Nazm-written compiler's subset until R2.
        E::Tuple { span, .. } => push_node(out, "tuple", 0, span.start, span.end, ""),
        // `print(e)` and `eprint(e)` read as `print(f"{e}")` (R1, 5b.13): the parser's rewrite,
        // whose one hole spans the call's parentheses, around the expression — a hole written in
        // `f"…"` lies inside its literal. The program as written is the expression.
        E::Interp { parts, span }
            if matches!(parts.as_slice(), [nazm_core::__syntax::InterpPart::Hole { span: hole, .. }]
                if hole.start < span.start && hole.end > span.end) =>
        {
            if let [nazm_core::__syntax::InterpPart::Hole { expr, .. }] = parts.as_slice() {
                dump_expr(expr, out);
            }
        }
        // An interpolation (L4), outside the Nazm-written compiler's subset until R2.
        E::Interp { span, .. } => push_node(out, "interp", 0, span.start, span.end, ""),
        // Gate 2's numbers, outside the Nazm-written compiler's subset: no agreement corpus
        // holds one.
        E::WideInt { value, span } => {
            push_node(out, "wide", 0, span.start, span.end, &value.to_string());
        }
        E::Float { text, span } => push_node(out, "float", 0, span.start, span.end, text),
        E::BitNot { operand, span, .. } => {
            dump_expr(operand, out);
            push_node(out, "bitnot", 1, span.start, span.end, "");
        }
        E::Str { value, span } => push_node(out, "str", 0, span.start, span.end, value),
        E::Var { name, span } => push_node(out, "var", 0, span.start, span.end, name),
        // The compiler written in Nazm has no closures (N50): its parser refuses one, so no
        // agreement corpus holds one.
        E::Closure { span, .. } => push_node(out, "closure", 0, span.start, span.end, ""),
        E::Construct {
            ty,
            type_args,
            fields,
            span,
            ..
        } => {
            // Initialisers in the order **written**, each named, because that is the
            // order both parsers must record: it is the order the values are computed in,
            // and a parser that sorted them would have changed the program's meaning.
            for f in fields {
                dump_expr(&f.value, out);
            }
            let names = fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            push_node(
                out,
                "construct",
                fields.len(),
                span.start,
                span.end,
                &format!("{ty}{}({names})", type_args_text(type_args.as_ref())),
            );
        }
        E::Variant {
            ty,
            type_args,
            variant,
            fields,
            span,
            ..
        } => {
            // Initialisers in the order **written**, exactly as a record construction's
            // are, and for the same reason.
            for f in fields {
                dump_expr(&f.value, out);
            }
            let names = fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            push_node(
                out,
                "makevariant",
                fields.len(),
                span.start,
                span.end,
                &format!(
                    "{ty}{}.{variant}({names})",
                    type_args_text(type_args.as_ref())
                ),
            );
        }
        E::Match {
            scrutinee,
            arms,
            span,
            ..
        } => {
            dump_expr(scrutinee, out);
            for a in arms {
                dump_expr(&a.body, out);
                // A plain variant pattern, the subset the compiler written in Nazm parses; any
                // other (R1a) is outside it until R2, and named so a source writing one differs.
                let text = match &a.pattern {
                    nazm_core::__syntax::Pattern::Variant(v) => {
                        let pattern = v
                            .fields
                            .iter()
                            .map(|f| match &f.pattern {
                                nazm_core::__syntax::Pattern::Bind { name, .. } => {
                                    format!("{}:{name}", f.name)
                                }
                                nazm_core::__syntax::Pattern::Wildcard { .. } => {
                                    format!("{}:_", f.name)
                                }
                                _ => format!("{}:<R1a pattern>", f.name),
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("{}.{}({pattern})", v.ty, v.variant)
                    }
                    _ => "<R1a pattern>".to_owned(),
                };
                push_node(out, "arm", 1, a.span.start, a.span.end, &text);
            }
            push_node(out, "match", arms.len() + 1, span.start, span.end, "");
        }
        E::Field {
            base, name, span, ..
        } => {
            dump_expr(base, out);
            push_node(out, "field", 1, span.start, span.end, name);
        }
        E::Neg { operand, span, .. } => {
            dump_expr(operand, out);
            push_node(out, "neg", 1, span.start, span.end, "");
        }
        // Not in the bootstrap subset (N49); named, so a source that wrote one would differ.
        E::Not { operand, span, .. } => {
            dump_expr(operand, out);
            push_node(out, "not", 1, span.start, span.end, "");
        }
        E::Propagate { operand, span, .. } => {
            dump_expr(operand, out);
            push_node(out, "propagate", 1, span.start, span.end, "");
        }
        E::Binary {
            op, lhs, rhs, span, ..
        } => {
            dump_expr(lhs, out);
            dump_expr(rhs, out);
            push_node(out, "bin", 2, span.start, span.end, op.symbol());
        }
        E::Call {
            callee,
            type_args,
            args,
            span,
            ..
        } => {
            for a in args {
                dump_expr(a, out);
            }
            let named = format!("{callee}{}", type_args_text(type_args.as_ref()));
            push_node(out, "call", args.len(), span.start, span.end, &named);
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
        // A method call (N78), which the compiler written in Nazm does not parse: no source
        // the parity tests read writes one, and the dump says so if one ever does.
        E::MethodCall { span, method, .. } => {
            push_node(out, "method-call", 0, span.start, span.end, method);
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

    // An enum and a `match`, pinned the same way. The arm's pattern travels as text
    // because that is what both parsers record: one span per arm, and the names inside it.
    let (e, errors) = nazm_core::__syntax::parse_source(
        "enum E { A, B(x: Int), }\nfn main() -> Int { match E.A() { E.A() => 1, E.B(x: n) => n, } }",
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let enum_dump = dump_program(&e);
    let enum_kinds: Vec<&str> = enum_dump
        .lines()
        .map(|l| l.split('|').next().unwrap_or(""))
        .collect();
    assert_eq!(
        enum_kinds,
        vec![
            "variant",
            "payload",
            "variant",
            "enum",
            "makevariant",
            "int",
            "arm",
            "var",
            "arm",
            "match",
            "block",
            "fn",
            "program",
        ],
        "{enum_dump}"
    );
    let patterns: Vec<&str> = enum_dump
        .lines()
        .filter(|l| l.starts_with("arm|"))
        .map(|l| l.rsplit('|').next().unwrap_or(""))
        .collect();
    assert_eq!(patterns, vec!["E.A()", "E.B(x:n)"], "{enum_dump}");

    // A discard is not the same pattern as a binding, or the format could not tell a
    // compiler that ignored `_` from one that honoured it.
    let (d, _) = nazm_core::__syntax::parse_source(
        "enum E { A, B(x: Int), }\nfn main() -> Int { match E.A() { E.A() => 1, E.B(x: _) => 0, } }",
    );
    assert_ne!(dump_program(&d), enum_dump, "the format cannot see `_`");
}

/// Build `compiler/parser.nz` natively and return the executable.
fn build_parser(dir: &Path) -> PathBuf {
    common::require_containment("parser.nz");
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
        // An expression suspended at an `if`'s block, where the block assigns, and then
        // resumed. The Nazm parser keeps a stack of operator-stack floors, one per
        // pending expression, and reads its top when it resumes — so a statement that
        // pushes more floors than it pops leaves a stale one on top for exactly this
        // shape. Nothing else in this list suspends an expression *across* an
        // assignment.
        "fn m() -> Int { let mut a = 0; let x = if true { a = 5; a } else { 3 }; x + a }",
        "fn m() -> Int { let x = if true { let mut y = 1; y = 2; y } else { 3 }; x }",
        "fn m(c: Bool) -> Int { let mut a = 1; let x = if c { a = a + 1; a } else { a = a * 2; a }; x + a }",
        // The same shape with an operator already pending when the `if` suspends: the
        // `+` sits on the operator stack *below* the `if`'s sentinel, so a floor that
        // came back too high would leave it unpopped and reassociate the expression.
        "fn m() -> Int { let mut a = 0; let x = 1 + if true { a = 5; a } else { 3 }; x + a }",
        "fn m(c: Bool) -> Int { let mut a = 2; let x = 1 + 2 * if c { a = 3; a } else { 4 } - 5; x + a }",
        // Structured concurrency. `scope` is a statement whose one child is its block;
        // `spawn` is a call node with the callee as its text and its arguments as its
        // children, spanning from the keyword to the `;`. The shapes are the ones
        // `dump_stmt` above writes down.
        "fn m() -> Int { scope { } 0 }",
        "fn w() -> Int { 0 }\nfn m() -> Int { scope { spawn w(); } 0 }",
        "fn w(a: Int, b: Int) -> Int { a + b }\nfn m() -> Int { scope { spawn w(1, 2 + 3); } 0 }",
        "fn w(c: Chan) -> Int { 0 }\nfn m() -> Int { let c = chan_new(1); scope { spawn w(c); spawn w(c); } 0 }",
        "fn w() -> Int { 0 }\nfn m() -> Int { scope { scope { spawn w(); } spawn w(); } 0 }",
        "fn w(n: Int) -> Int { n }\nfn m() -> Int { let mut i = 0; scope { while i < 3 { spawn w(i); i = i + 1; } } i }",
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
        // N11: type parameters on every kind of declaration, applied types in every
        // position a type is written, and type arguments after a name in an expression —
        // including an empty list, which parses and is the checker's to refuse.
        "struct Pair[A, B] { first: A, second: B, }\nfn m() -> Int { 0 }",
        "enum Maybe[T] { None, Some(value: T), }\nfn m() -> Int { 0 }",
        "fn identity[T](value: T) -> T { value }",
        // N14: a requirement, on one parameter of several, with the spacing varied.
        "fn same[T: Equality, U, V :Equality](a: T, b: U, c: V) -> Bool { a == a }",
        "fn m(p: Pair[Int, Vec[Str]]) -> Vec[Pair[Int, Str]] { f(p) }",
        "struct Box[T] { v: Vec[Maybe[T]], }\nfn m() -> Int { 0 }",
        "fn m() -> Int { identity[Int](7) }",
        "fn m() -> Int { let p = Pair[Int, Str](first: 1, second: \"x\"); p.first }",
        "fn m() -> Int { let x = Maybe[Int].Some(value: 42); let y = Maybe[Int].None(); 0 }",
        "fn m() -> Int { let xs = vec_new[Pair[Int, Vec[Str]]](); vec_len(xs) }",
        "fn m() -> Int { f[](1) + g[Vec[Vec[Int]], Str](2) }",
        "fn m() -> Int { match Maybe[Int].None() { Maybe.None() => 0, Maybe.Some(value: v) => v, } }",
        "fn m() -> Int { h(identity[Str](\"a\"), Box[Int](v: vec_new[Maybe[Int]]())) }",
        // N12: `?` as a postfix beside projection, in any order and any number, binding
        // tighter than negation and than every binary operator.
        "fn m(r: Result[Int, Str]) -> Result[Int, Str] { let a = r?; Result[Int, Str].Ok(value: a) }",
        "fn m() -> Int { -f()?.x + g(h()??, (k()?)?) }",
        "fn m() -> Int { f(1)? * 2 - x?.y.z? }",
        "fn m() -> Int { match load()? { A.B() => 1, A.C(v: v) => v?, } }",
        // N12.1: a block where an expression is — an arm, an initialiser, an argument, an
        // operand, a statement — which is `block` wrapped in `blockexpr`, as the dump above
        // writes the reference's `Expr::Block`.
        "fn m() -> Int { { 1 } }",
        "fn m() -> Int { let x = { let a = 2; a * 3 }; x }",
        "fn m() -> Int { f({ 1 }, { let b = 2; b }) + { 3 } * -{ 4 } }",
        "fn m() -> Int { match o { A.B() => { let x = 1; x }, A.C(v: v) => { v }, } }",
        "fn m() -> Int { match o { A.B() => { return 1; }, A.C(v: v) => v, } }",
        "fn m() -> Int { let mut x = 0; { x = 1; } { let y = 2; x = y; } x }",
        "fn m() -> Int { P(a: { 1 }, b: E.V(c: { let d = 2; d })).a }",
        "fn m() -> Int { ({ r })?.x + { { 5 } } }",
        "fn m() -> Int { if { true } { { 1 } } else { 2 } }",
        "fn m() -> Int { while { false } { { break; } } 0 }",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, text, stderr) = run_stage(&exe, &file);
        assert_eq!(
            status,
            Some(0),
            "the Nazm parser failed on {source:?}: {stderr}"
        );
        // The driver prints the node count as `main`'s value; the dump is everything
        // before it.
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
    // The reference parses whole files, recursing with their shape: on the stack the parser
    // documents (`PARSE_STACK_BYTES`), not a test thread's default.
    std::thread::Builder::new()
        .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
        .spawn(the_nazm_parser_agrees_on_a_parse_stack)
        .expect("a thread for the parser")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
}

fn the_nazm_parser_agrees_on_a_parse_stack() {
    // The real test of a parser: whole files, including its own source and the
    // tokeniser's — the largest and most varied Nazm programs that exist.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_parser(dir.path());
    let root = repo_root();

    let mut files = Vec::new();
    for folder in ["compiler", "examples", "library/core"] {
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

        // `scope` and `spawn` are no longer skipped or asserted to be refused here: the
        // Nazm parser handles them, and `examples/worker-pool.nz` is compared tree for
        // tree like every other file. The checker is a separate question — see
        // `the_nazm_checker_agrees_with_the_reference_on_names_and_types`.
        let (status, text, stderr) = run_stage(&exe, file);
        assert_eq!(
            status,
            Some(0),
            "the Nazm parser failed on {}: {stderr}",
            file.display()
        );
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
            "fn m() -> Int { enum E { A } 0 }",
            "an enum declared inside a function: not implemented yet",
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
        let (status, _, stderr) = run_stage(&exe, &file);
        assert_eq!(status, Some(1), "{source:?} should be refused");
        assert!(
            stderr.contains(expected),
            "{source:?}: expected {expected:?}, got {stderr}"
        );
        assert!(stderr.contains("parse error at "), "{source:?}: {stderr}");
    }
}

/// Build `compiler/check.nz` natively and return the executable.
fn build_checker(dir: &Path) -> PathBuf {
    common::require_containment("check.nz");
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

/// The diagnostics both checkers are expected to produce for the same source.
///
/// Not every code: the Nazm-written checker is a checker for the bootstrap subset and does
/// not implement the lexer's or parser's codes, or the ones for constructs it never sees.
/// The list is what the two **do** both decide, and comparing a subset is only honest
/// while the subset is stated — so it is one constant, used on both sides of every
/// comparison, rather than a filter applied to whichever side is inconvenient.
///
/// `N0203`, `N0204`, `N0206`–`N0208` joined it on 2026-09-22 (N2.1), when the Nazm checker
/// learned module ownership and visibility. Before that it could not produce them, and
/// leaving them out of the reference's side was hiding a divergence rather than scoping a
/// comparison.
///
/// `N0313` joined it in N10.1, when the Nazm checker learned which children never
/// complete. Until then it accepted `continue; i = i + 10;` and the emitter compiled the
/// statement the reference refuses.
///
/// `N0302` joined it in N10.2, when the Nazm checker learned the reference's `if` join —
/// two values must agree, and a value with nothing is refused — and `N0300` began to cover
/// what it had never covered: a value position given nothing, and a body or a `return`
/// against its signature.
const COMPARABLE: &[&str] = &[
    "N0200", "N0201", "N0203", "N0204", "N0206", "N0207", "N0208", "N0300", "N0301", "N0303",
    "N0302", "N0304", "N0310", "N0313", "N0320", "N0321", "N0369",
];

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_names_and_types() {
    // **Scope, stated:** `compiler/check.nz` reports `N0200`, `N0201`, `N0300`, `N0301`,
    // `N0302`, `N0303`, `N0304`, `N0310` and `N0313` among the codes compared here — no `N0204`, no unreachable-code
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

        // --- concurrency ---------------------------------------------------------
        // Clean: a `Chan` crosses into a task, which is the one handle that may.
        "fn w(c: Chan) -> Int { let b = ints_new(); chan_recv(c, b); ints_pop(b) }\nfn m() -> Int { let c = chan_new(1); scope { spawn w(c); chan_send(c, 7); } 0 }",
        "fn w() -> Int { 0 }\nfn m() -> Int { scope { } scope { spawn w(); spawn w(); } 0 }",
        // A task needs a scope to join it (N0320).
        "fn w() -> Int { 0 }\nfn m() -> Int { spawn w(); 0 }",
        // A sequence is shared mutable storage with no synchronisation, so it may not
        // cross into a task (N0321) — a channel may.
        "fn w(xs: Ints) -> Int { ints_len(xs) }\nfn m() -> Int { let xs = ints_new(); scope { spawn w(xs); } 0 }",
        "fn w(a: Strs) -> Int { strs_len(a) }\nfn m() -> Int { let a = strs_new(); scope { spawn w(a); } 0 }",
        // The channel built-ins are typed: a `Chan` where an `Int` belongs is N0303.
        "fn m() -> Int { let c = chan_new(1); chan_send(c, c); 0 }",

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
        // Ordering across two types; equality works on one type but not across two. (Ordering
        // two strings or two `Bool`s is the 1.0 draft's since R1a, L6, and outside this
        // compiler's subset until R2.)
        "fn m() -> Bool { \"a\" < 1 }",
        "fn m() -> Bool { true < 1 }",
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

        // --- completion (N10.1) ----------------------------------------------------
        // Clean: a branch that leaves contributes no type, so the other one decides —
        // the Nazm checker took the then-branch's `unknown` as the whole `if`'s type.
        "fn m() -> Int { let mut i = 0; let mut t = 0; while i < 3 { i = i + 1; t = t + if i == 1 { continue; } else { i }; } t }",
        "fn m() -> Int { let mut i = 0; let mut t = 0; while i < 3 { i = i + 1; t = t + if i == 1 { i } else { break; }; } t }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; let v = if i == 1 { continue; } else { break; }; } i }",
        // What follows a statement that leaves can never run: once per run of them, and
        // a tail after one is reported too.
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; if i > 1 { continue; } else { break; }; i = i + 10; } i }",
        "fn m() -> Int { return 1; 2 }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; break; let a = 1; let b = 2; } i }",
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; continue; i = 5; break; i = 6; } i }",
        // A value that never arrives, where nothing says what it should be, is `Int`: so
        // comparing it with a `Str` is refused, exactly as the reference refuses it.
        "fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; let b = (if i == 1 { continue; } else { break; }) == \"a\"; } i }",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, _) = run_stage(&exe, &file);
        let ours = sorted_diagnostics(&stdout);

        let theirs: Vec<(String, u32, u32)> = {
            let mut v: Vec<(String, u32, u32)> = nazm_core::check_source(source)
                .into_iter()
                .filter(|d| COMPARABLE.contains(&d.code.as_str()))
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
            status,
            Some(i32::from(!theirs.is_empty())),
            "wrong exit status for {source:?}"
        );
    }
}

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_what_produces_a_value() {
    // N10.2. The reference's `Completion` has three states — a value, **no** value, and
    // never finishing — and the Nazm checker had two: `unknown` stood for "no value" as
    // well as for "already reported", so a value position given nothing was accepted and
    // the emitter allocated `alloca void`, and an `if` whose branches disagreed was typed
    // by one of them and handed clang a mistyped `phi`. Refused programs only — which is
    // why no accepted-language test could see it.
    //
    // Every case in `tests/refusals/` is checked four ways, and all four must agree on
    // code **and** span: the reference, the Nazm checker compiled and interpreted, and a
    // written expectation. The Nazm compiler must then refuse it by the checker's code —
    // exit 1, before emission — or accept it, for the controls whose name starts `ok_`.
    let dir = tempfile::tempdir().expect("tempdir");
    let checker = build_checker(dir.path());
    let emitter = build_emitter(dir.path());
    let corpus = repo_root().join("crates/nazm-cli/tests/refusals");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .expect("read the refusal corpus")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "nz"))
        .collect();
    cases.sort();
    assert!(cases.len() >= 40, "only {} refusal cases", cases.len());

    for file in &cases {
        let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let source = std::fs::read_to_string(file).expect("read case");
        let written = std::fs::read_to_string(file.with_extension("expected"))
            .expect("every case has an .expected");
        let written: Vec<String> = match written.trim() {
            "ok" => Vec::new(),
            codes => codes.split_whitespace().map(ToOwned::to_owned).collect(),
        };

        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(&source)
            .into_iter()
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        let codes: Vec<String> = theirs.iter().map(|(c, _, _)| c.clone()).collect();
        assert_eq!(
            codes, written,
            "{name}: the reference no longer says what was written"
        );

        let (status, compiled, _) = run_stage(&checker, file);
        assert_eq!(
            sorted_diagnostics(&compiled),
            theirs,
            "{name}: the compiled checker differs"
        );
        assert_eq!(
            status,
            Some(i32::from(!theirs.is_empty())),
            "{name}: the compiled checker's exit status"
        );
        let interpreted = Command::new(NAZM)
            .args([
                "run",
                "--interpret",
                "compiler/check.nz",
                "--",
                file.to_str().unwrap(),
                "library/core/prelude.nz",
            ])
            .current_dir(repo_root())
            .output()
            .expect("interpret the Nazm checker");
        assert_eq!(
            sorted_diagnostics(&String::from_utf8_lossy(&interpreted.stdout)),
            theirs,
            "{name}: the interpreted checker differs"
        );

        // And the refusal happens **before** emission: the compiler stops with the
        // checker's code rather than writing IR that clang or the runtime would find wrong.
        let (rc, ir, err) = run_stage(&emitter, file);
        if theirs.is_empty() {
            assert_eq!(
                rc,
                Some(0),
                "{name}: the Nazm compiler refused a control:\n{err}"
            );
            // A control is also run, against the interpreter: several are statements whose
            // `match` produces nothing while an arm produces a value, which the emitter
            // must discard — and give back — rather than leave for a later node to take.
            let ll = dir.path().join(format!("{name}.ll"));
            std::fs::write(&ll, &ir).expect("write IR");
            let exe = dir.path().join(format!("{name}-selfhost"));
            let assembled = Command::new("clang")
                .args(["-Wno-override-module", "-lpthread", "-o"])
                .arg(&exe)
                .arg(&ll)
                .output()
                .expect("run clang");
            assert!(
                assembled.status.success(),
                "{name}: clang rejected the Nazm compiler's IR:\n{}",
                String::from_utf8_lossy(&assembled.stderr)
            );
            let (code, out, run_err) = run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                std::time::Duration::from_secs(60),
            )
            .expect("the compiled control hung");
            let interp = Command::new(NAZM)
                .args(["run", "--interpret", file.to_str().unwrap()])
                .env(nazm_lir::MEMORY_VARIABLE, "1")
                .output()
                .expect("run nazm run");
            assert_eq!(code, Some(0), "{name}: {run_err}");
            assert_eq!(
                out.trim(),
                String::from_utf8_lossy(&interp.stdout).trim(),
                "{name}: the compiled control disagrees with the interpreter"
            );
            let memory = |text: &str| {
                text.lines()
                    .find(|l| l.starts_with("nazm-memory:"))
                    .map(ToOwned::to_owned)
            };
            let line = memory(&run_err);
            assert_eq!(
                line,
                memory(&String::from_utf8_lossy(&interp.stderr)),
                "{name}"
            );
            assert!(
                line.as_deref()
                    .is_some_and(|l| l.matches("live=0").count() == 4),
                "{name}: left something behind: {line:?}"
            );
        } else {
            assert_eq!(
                rc,
                Some(1),
                "{name}: the Nazm compiler did not refuse:\n{err}"
            );
            let code = selfhost_code(&err).unwrap_or_default();
            assert!(
                codes.contains(&code),
                "{name}: refused as {code}, not {codes:?}"
            );
        }
    }

    // Nominal identity across a module boundary: two records laid out alike, exported by
    // two modules, are two types. The Nazm checker's driver sees one file, so the Nazm
    // compiler — which resolves imports itself — is what is compared, in both forms.
    for (case, want) in [("same-shape", Some("N0302")), ("same-module-ok", None)] {
        let main = corpus.join("modules").join(case).join("main.nz");
        let reference = Command::new(NAZM)
            .args(["check", "--no-cache", main.to_str().unwrap()])
            .output()
            .expect("run nazm check");
        let ref_err = String::from_utf8_lossy(&reference.stderr).into_owned();
        assert_eq!(
            reference_code(&ref_err).as_deref(),
            want,
            "{case}: reference:\n{ref_err}"
        );
        let (rc, _, err) = run_stage(&emitter, &main);
        let interpreted = Command::new(NAZM)
            .args([
                "run",
                "--interpret",
                "compiler/emit.nz",
                "--",
                main.to_str().unwrap(),
                "library/core/prelude.nz",
            ])
            .current_dir(repo_root())
            .output()
            .expect("interpret the Nazm compiler");
        let int_err = String::from_utf8_lossy(&interpreted.stderr).into_owned();
        assert_eq!(
            selfhost_code(&err).as_deref(),
            want,
            "{case}: compiled:\n{err}"
        );
        assert_eq!(
            selfhost_code(&int_err).as_deref(),
            want,
            "{case}: interpreted:\n{int_err}"
        );
        assert_eq!(rc, Some(i32::from(want.is_some())), "{case}: exit status");
        if want.is_some() {
            // The same position, `line:column`, in all three.
            assert!(
                ref_err.contains("main.nz:6:38"),
                "{case}: reference position:\n{ref_err}"
            );
            assert!(
                err.contains("main.nz:6:38"),
                "{case}: compiled position:\n{err}"
            );
            assert!(
                int_err.contains("main.nz:6:38"),
                "{case}: interpreted position:\n{int_err}"
            );
        }
    }
}

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_enums_and_matching() {
    // **Codes, not spans**, and stated rather than implied: the self-hosted parser records
    // one span per `match` arm, so a diagnostic about a payload field inside a pattern
    // points at the arm where the reference points at the field. That is a difference in
    // where a message points, not in what either compiler decides — and *what each
    // decides* is what N2.1 parity is about, so it is what this compares.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());

    let sources = [
        // Clean, and exercising the whole of N10: every variant shape, both directions of
        // composition, nesting, a discard, a returned payload, and a borrowed handle.
        "enum E { A, } fn m() -> Int { match E.A() { E.A() => 0, } }",
        "enum E { B(x: Int), A, } fn m() -> Int { match E.A() { E.A() => 0, E.B(x: n) => n, } }",
        "enum E { V(l: Int, r: Int), } fn m() -> Int { match E.V(l: 1, r: 2) { E.V(l: a, r: b) => a + b, } }",
        "enum E { V(l: Int, r: Int), } fn m() -> Int { match E.V(r: 2, l: 1) { E.V(r: b, l: a) => a + b, } }",
        "enum E { V(x: Int), } fn m() -> Int { match E.V(x: 1) { E.V(x: _) => 0, } }",
        "enum E { T(v: Str), N, } fn t(e: E) -> Str { match e { E.N() => \"\", E.T(v: s) => s, } }\nfn m() -> Int { str_len(t(E.T(v: \"ab\"))) }",
        "enum E { S(xs: Ints), } fn m() -> Int { let a = ints_new(); match E.S(xs: a) { E.S(xs: b) => ints_push(b, 1), }; ints_len(a) }",
        "struct P { x: Int, } enum S { At(p: P), D, } fn m() -> Int { match S.At(p: P(x: 3)) { S.D() => 0, S.At(p: q) => q.x, } }",
        "enum I { A(n: Int), B, } struct E { s: I, } fn m() -> Int { let e = E(s: I.A(n: 4)); match e.s { I.B() => 0, I.A(n: n) => n, } }",
        "enum I { A, B, } enum O { W(v: I), E, } fn m() -> Int { match O.W(v: I.A()) { O.E() => 0, O.W(v: v) => match v { I.A() => 1, I.B() => 2, }, } }",
        "enum A { R, } enum B { R, } fn m() -> Int { match A.R() { A.R() => 1, } + match B.R() { B.R() => 2, } }",
        "enum E { A, } fn E() -> Int { 4 } fn m() -> Int { E() + match E.A() { E.A() => 5, } }",
        "pub enum E { A(v: Str), } pub fn f(e: E) -> Int { match e { E.A(v: s) => str_len(s), } }\nfn m() -> Int { f(E.A(v: \"a\")) }",
        "enum M { T(v: Str), C(n: Int), } fn w(e: M) -> Int { match e { M.T(v: s) => str_len(s), M.C(n: n) => n, } }\nfn m() -> Int { let c = chan_new(1); scope { } w(M.T(v: \"a\")) }",

        // Now the refusals, one rule at a time.
        "enum E { A, B, } fn m() -> Int { match E.A() { E.A() => 1, } }",
        "enum E { A, B, } fn m() -> Int { match E.A() { E.A() => 1, E.A() => 2, E.B() => 3, } }",
        "enum A { R, } enum B { R, } fn m() -> Int { match A.R() { B.R() => 1, } }",
        "enum E { A, } fn m() -> Int { match E.Z() { E.A() => 1, } }",
        "enum E { A, } fn m() -> Int { match E.A() { E.Z() => 1, } }",
        "fn m() -> Int { match 42 { } }",
        "struct P { x: Int, } fn m() -> Int { match P(x: 1) { } }",
        "enum E { V(x: Int, y: Int), } fn m() -> Int { match E.V(x: 1, y: 2) { E.V(x: a) => a, } }",
        "enum E { V(x: Int), } fn m() -> Int { match E.V(x: 1) { E.V(y: a) => a, } }",
        "enum E { V(x: Int), } fn m() -> Int { match E.V(x: 1, y: 2) { E.V(x: a) => a, } }",
        "enum E { V(x: Int, y: Int), } fn m() -> Int { match E.V(x: 1) { E.V(x: a, y: b) => a + b, } }",
        "enum E { V(x: Int), } fn m() -> Int { match E.V(x: \"s\") { E.V(x: a) => a, } }",
        "enum E { A(x: Int), } fn m() -> Int { E.A(x: 1).x }",
        "enum E { A, } fn m() -> Int { let a = E.A(); let b = E.A(); if a == b { 1 } else { 0 } }",
        "enum E { A, A, } fn m() -> Int { match E.A() { E.A() => 1, } }",
        "enum E { V(x: Int, x: Int), } fn m() -> Int { 0 }",
        "enum E {} fn m() -> Int { 0 }",
        "struct I { x: Int, } enum I { A, } fn m() -> Int { 0 }",
        "enum Int { A, } fn m() -> Int { 0 }",
        "enum L { E, N(r: L), } fn m() -> Int { 0 }",
        "struct A { b: B, } enum B { V(a: A), } fn m() -> Int { 0 }",
        "struct S { x: Int, } pub enum P { H(v: S), } fn m() -> Int { 0 }",
        "enum W { N, V(xs: Ints), } fn w(e: W) -> Int { match e { W.N() => 0, W.V(xs: x) => ints_len(x), } }\nfn m() -> Int { scope { spawn w(W.N()); } 0 }",
        "enum E { A, B, } fn m() -> Int { let n = match E.A() { E.A() => 1, E.B() => \"t\", }; n }",
        "enum E { A(x: Int, y: Int), } fn m() -> Int { match E.A(x: 1, y: 2) { E.A(x: a, y: a) => a, } }",
        "enum E { A(x: Int), } fn m() -> Int { match E.A(x: 1) { E.A(x: v) => v, } + v }",
        // N10.1: a scrutinee that is an `if` with one branch leaving is the other branch's
        // enum, so its payload names are bound — and one with both leaving is an `Int`,
        // which a `match` refuses.
        "enum E { A, T(v: Str), } fn m() -> Int { let mut i = 0; let mut t = 0; while i < 3 { i = i + 1; t = t + match (if i == 1 { continue; } else { E.T(v: \"ab\") }) { E.A() => 0, E.T(v: s) => str_len(s), }; } t }",
        "enum E { A, B, } fn m() -> Int { let mut i = 0; while i < 3 { i = i + 1; let n = match (if i == 1 { continue; } else { break; }) { E.A() => 1, E.B() => 2, }; } i }",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, _) = run_stage(&exe, &file);
        let mut ours: Vec<String> = stdout
            .lines()
            .filter(|l| l.contains('|'))
            .filter_map(|l| l.split('|').next().map(ToOwned::to_owned))
            .collect();
        ours.sort();

        let mut theirs: Vec<String> = nazm_core::check_source(source)
            .into_iter()
            .map(|d| d.code.as_str().to_owned())
            .collect();
        theirs.sort();

        assert_eq!(ours, theirs, "checking differs for {source:?}");
        assert_eq!(
            status,
            Some(i32::from(!theirs.is_empty())),
            "wrong exit status for {source:?}"
        );
    }
}

/// The generic codes both checkers decide, compared on code **and** span — N11. With them the
/// codes of the rules a generic program most often meets on the way: a mismatch, an
/// unknown type, an operator, a task capture.
const GENERIC_COMPARABLE: &[&str] = &[
    "N0101", "N0300", "N0304", "N0305", "N0321", "N0331", "N0336", "N0338", "N0350", "N0351",
    "N0352", "N0353", "N0354", "N0355", "N0356", "N0359",
];

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_generics() {
    // N11, `docs/spec.md`, *Generics*. Accepted programs must be accepted by both, and every
    // refused one must be refused with the same codes at the same spans: a generic checker
    // that agreed only on the programs it accepts would agree with everything.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());

    let sources = [
        // Clean.
        "struct Pair[A, B] { first: A, second: B, }\nfn m() -> Int { let p = Pair[Int, Str](first: 1, second: \"x\"); p.first }",
        "enum Maybe[T] { None, Some(value: T), }\nfn or[T](m: Maybe[T], d: T) -> T { match m { Maybe.None() => d, Maybe.Some(value: v) => v, } }\nfn m() -> Int { or(Maybe[Int].Some(value: 4), 0) }",
        "fn identity[T](value: T) -> T { value }\nfn m() -> Int { identity[Int](7) + identity(3) + str_len(identity(\"ab\")) }",
        "fn m() -> Int { let xs = vec_new[Vec[Str]](); vec_push(xs, vec_new[Str]()); vec_len(vec_get(xs, 0)) }",
        "struct Tag[T] { n: Int, }\nstruct S { t: Tag[S], xs: Vec[Vec[Int]], }\nfn m() -> Int { 0 }",
        "struct Box[T] { v: T, }\nfn un[T](b: Box[T]) -> T { b.v }\nfn m() -> Int { un(Box[Int](v: 3)) }",
        "fn first[T](xs: Vec[T]) -> T { vec_get(xs, 0) }\nfn m() -> Int { let v = vec_new[Int](); vec_push(v, 1); first(v) }",
        "fn swap[A, B](a: A, b: B) -> B { b }\nfn m() -> Int { swap(\"a\", 1) }",
        "enum L[T] { Nil, Cons(head: T, tail: Vec[Int]), }\nfn m() -> Int { 0 }",
        // Refused: the declaration rules.
        "fn f[T, T](x: T) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f[Int](x: Int) -> Int { 0 }\nfn m() -> Int { 0 }",
        "struct B[Vec] { v: Int, }\nfn m() -> Int { 0 }",
        "fn main[T]() -> Int { 0 }",
        // Refused: written types.
        "struct Box[T] { v: T, }\nfn f(b: Box) -> Int { 0 }\nfn m() -> Int { 0 }",
        "struct Box[T] { v: T, }\nfn f(b: Box[Int, Str]) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f(b: Int[Str]) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f[T](x: T[Int]) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f(v: Vec) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f(v: Vec[Int, Int]) -> Int { 0 }\nfn m() -> Int { 0 }",
        "fn f(v: Vec[Nope]) -> Int { 0 }\nfn m() -> Int { 0 }",
        "struct P { x: Int, }\nfn f(p: P[Int]) -> Int { 0 }\nfn m() -> Int { 0 }",
        // Refused: constructions and variants.
        "struct Box[T] { v: T, }\nfn m() -> Int { let b = Box(v: 1); 0 }",
        "struct Box[T] { v: T, }\nfn m() -> Int { let b = Box[Int, Str](v: 1); 0 }",
        "struct P { x: Int, }\nfn m() -> Int { let p = P[Int](x: 1); p.x }",
        "enum Maybe[T] { None, Some(value: T), }\nfn m() -> Int { let x = Maybe.None(); 0 }",
        "enum Maybe[T] { None, Some(value: T), }\nfn m() -> Int { let x = Maybe[Int].Some(value: \"s\"); 0 }",
        // Refused: calls.
        "fn f(x: Int) -> Int { x }\nfn m() -> Int { f[Int](1) }",
        "fn m() -> Int { let xs = vec_new(); 0 }",
        "fn same[T](a: T, b: T) -> T { a }\nfn m() -> Int { same(1, \"x\") }",
        "fn f[T](xs: Vec[T]) -> Int { 0 }\nfn m() -> Int { f(1) }",
        "fn id[T](x: T) -> T { x }\nfn m() -> Int { id[Int, Str](1) }",
        "fn id[T](x: T) -> T { x }\nfn m() -> Int { id[Str](1) }",
        "fn id[T](x: T) -> T { x }\nfn m() -> Str { id(1) }",
        // Refused: what an unconstrained `T` supports.
        "fn f[T](a: T, b: T) -> Bool { a == b }\nfn m() -> Int { 0 }",
        "fn f[T](a: T) -> Int { a + 1 }\nfn m() -> Int { 0 }",
        "fn f[T](a: T) -> Int { a.x }\nfn m() -> Int { 0 }",
        // Refused: tasks.
        "fn w(v: Vec[Int]) -> Int { 0 }\nfn m() -> Int { let v = vec_new[Int](); scope { spawn w(v); } 0 }",
        "fn w[T](v: T) -> Int { 0 }\nfn m() -> Int { scope { spawn w(1); } 0 }",
        // Refused: ownership.
        "struct Node { children: Vec[Node], }\nfn m() -> Int { 0 }",
        "struct Box[T] { v: T, }\nstruct S { b: Box[S], }\nfn m() -> Int { 0 }",
        "struct A { bs: Vec[B], }\nenum B { V(a: A), N, }\nfn m() -> Int { 0 }",
        "struct Strange[T] { next: Vec[Strange[Vec[T]]], }\nfn m() -> Int { 0 }",
    ];

    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout)
            .into_iter()
            .filter(|(c, _, _)| GENERIC_COMPARABLE.contains(&c.as_str()))
            .collect();
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .filter(|d| GENERIC_COMPARABLE.contains(&d.code.as_str()))
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        let refused = !nazm_core::check_source(source).is_empty();
        assert_eq!(
            status,
            Some(i32::from(refused)),
            "wrong exit status for {source:?}"
        );
    }
}

/// The codes the N12 comparison holds both checkers to: propagation's four, and the ones
/// `Result` and `Option` raise as the ordinary enums they are.
const PROPAGATION_COMPARABLE: &[&str] = &[
    "N0300", "N0321", "N0336", "N0359", "N0360", "N0361", "N0362", "N0363",
];

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_propagation() {
    // N12, `docs/spec.md`, *The core prelude* and *Propagation*. Both checkers load the
    // same prelude source, and must agree on every accepted program and on every refusal's
    // code and span — the `?` itself for propagation's own codes, the declaration for a
    // reserved name.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let oops = "struct Oops { code: Int, }\nfn leaf(f: Bool) -> Result[Int, Oops] { \
                if f { return Result[Int, Oops].Err(error: Oops(code: 1)); } Result[Int, Oops].Ok(value: 2) }\n";
    let with = |body: &str| format!("{oops}{body}");

    let sources = [
        // Clean.
        with("fn f(x: Bool) -> Result[Int, Oops] { let n = leaf(x)?; Result[Int, Oops].Ok(value: n) }\nfn m() -> Int { 0 }"),
        with("fn g() -> Result[Str, Oops] { Result[Str, Oops].Ok(value: int_to_str(leaf(true)? + 1)) }\nfn m() -> Int { 0 }"),
        with("fn g() -> Result[Int, Oops] { let mut t = 0; while t < 3 { t = t + leaf(false)?; } Result[Int, Oops].Ok(value: t) }\nfn m() -> Int { 0 }"),
        "fn pass[T, E](r: Result[T, E]) -> Result[T, E] { let v = r?; Result[T, E].Ok(value: v) }\nfn m() -> Int { 0 }".to_owned(),
        "fn i(n: Int) -> Result[Result[Int, Str], Str] { Result[Result[Int, Str], Str].Ok(value: Result[Int, Str].Ok(value: n)) }\nfn g() -> Result[Int, Str] { let a = i(1)??; let b = (i(2)?)?; Result[Int, Str].Ok(value: a + b) }\nfn m() -> Int { 0 }".to_owned(),
        "struct P { x: Int, }\nfn mk() -> Result[P, Str] { Result[P, Str].Ok(value: P(x: 1)) }\nfn g() -> Result[Int, Str] { Result[Int, Str].Ok(value: -mk()?.x) }\nfn m() -> Int { 0 }".to_owned(),
        "fn get(o: Option[Int]) -> Int { match o { Option.None() => 0, Option.Some(value: n) => n, } }\nfn m() -> Int { get(Option[Int].Some(value: 4)) }".to_owned(),
        "fn w(r: Result[Int, Str]) -> Int { 0 }\nfn m() -> Int { scope { spawn w(Result[Int, Str].Ok(value: 1)); } 0 }".to_owned(),
        // Refused: `?` on what is not the core `Result`.
        "fn g() -> Result[Int, Str] { let n = 4?; Result[Int, Str].Ok(value: n) }\nfn m() -> Int { 0 }".to_owned(),
        "fn g(o: Option[Int]) -> Result[Int, Str] { let n = o?; Result[Int, Str].Ok(value: n) }\nfn m() -> Int { 0 }".to_owned(),
        "enum MyResult[T, E] { Ok(value: T), Err(error: E), }\nfn g(r: MyResult[Int, Str]) -> Result[Int, Str] { let n = r?; Result[Int, Str].Ok(value: n) }\nfn m() -> Int { 0 }".to_owned(),
        // Refused: `?` where the function does not return the core `Result`.
        "fn g(r: Result[Int, Str]) -> Int { r? }\nfn m() -> Int { 0 }".to_owned(),
        "fn main() -> Int { Result[Int, Str].Ok(value: 1)? }".to_owned(),
        "enum MyResult[T, E] { Ok(value: T), Err(error: E), }\nfn g(r: Result[Int, Str]) -> MyResult[Int, Str] { MyResult[Int, Str].Ok(value: r?) }\nfn m() -> Int { 0 }".to_owned(),
        // Refused: a different error type, nominally — even one with the same fields.
        "struct A { c: Int, }\nstruct B { c: Int, }\nfn g(r: Result[Int, A]) -> Result[Int, B] { let n = r?; Result[Int, B].Ok(value: n) }\nfn m() -> Int { 0 }".to_owned(),
        "fn g[T, E1, E2](r: Result[T, E1]) -> Result[T, E2] { let v = r?; Result[T, E2].Ok(value: v) }\nfn m() -> Int { 0 }".to_owned(),
        // Refused: the success value used as the wrong type.
        "fn g(r: Result[Int, Str]) -> Result[Str, Str] { Result[Str, Str].Ok(value: r?) }\nfn m() -> Int { 0 }".to_owned(),
        // Refused: a prelude name declared.
        "enum Result[T, E] { Ok(value: T), Err(error: E), }\nfn m() -> Int { 0 }".to_owned(),
        "struct Option { x: Int, }\nfn m() -> Int { 0 }".to_owned(),
        // Refused: the ordinary enum laws, through the prelude's types.
        "struct Node { next: Option[Vec[Node]], }\nfn m() -> Int { 0 }".to_owned(),
        "struct Node { next: Option[Node], }\nfn m() -> Int { 0 }".to_owned(),
        "struct D { c: Str, }\nfn w(r: Result[Int, Vec[D]]) -> Int { 0 }\nfn m() -> Int { let r = Result[Int, Vec[D]].Ok(value: 1); scope { spawn w(r); } 0 }".to_owned(),
    ];

    let mut refusals = 0;
    for source in &sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout)
            .into_iter()
            .filter(|(c, _, _)| PROPAGATION_COMPARABLE.contains(&c.as_str()))
            .collect();
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .filter(|d| PROPAGATION_COMPARABLE.contains(&d.code.as_str()))
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        if !theirs.is_empty() {
            refusals += 1;
            assert_eq!(status, Some(1), "{source:?} should have been refused");
        }
    }
    assert!(refusals >= 14, "only {refusals} refusals were compared");
}

#[test]
fn the_nazm_written_compiler_propagates_with_the_same_memory_semantics() {
    // N12: `?` compiled by the Nazm-written compiler, against the reference, on the
    // shapes where getting the order of a retain and a release wrong shows — an error
    // value leaving through three functions, a success payload outliving the `Result` it
    // came out of, a half-evaluated call and a half-built record abandoned, a scope's task
    // joined on the way out, and a generic function's instances.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let churn = "fn churn() -> Int { let mut noise = vec_new[Str](); let mut k = 0; \
                 while k < 64 { vec_push(noise, str_concat(\"noise-\", int_to_str(k))); k = k + 1; } \
                 noise = vec_new[Str](); vec_len(noise) }\n";
    let cases: Vec<(String, &str)> = vec![
        // An error value, a record and a `Vec[Diag]` propagated twice; a success `Str`.
        (format!("{churn}struct Item {{ name: Str, tags: Ints, }}\nstruct Diag {{ code: Str, at: Int, }}\n\
             fn item(n: Int) -> Item {{ let t = ints_new(); ints_push(t, n); Item(name: str_concat(\"item-\", int_to_str(n)), tags: t) }}\n\
             fn diags(n: Int) -> Vec[Diag] {{ let d = vec_new[Diag](); let mut i = 0; while i < n {{ vec_push(d, Diag(code: int_to_str(300 + i), at: i)); i = i + 1; }} d }}\n\
             fn ls() -> Result[Int, Str] {{ Result[Int, Str].Err(error: str_concat(\"broken-\", int_to_str(42))) }}\n\
             fn lr() -> Result[Int, Item] {{ Result[Int, Item].Err(error: item(31)) }}\n\
             fn lv() -> Result[Int, Vec[Diag]] {{ Result[Int, Vec[Diag]].Err(error: diags(5)) }}\n\
             fn ms() -> Result[Int, Str] {{ let n = ls()?; churn(); Result[Int, Str].Ok(value: n) }}\n\
             fn mr() -> Result[Int, Item] {{ let n = lr()?; Result[Int, Item].Ok(value: n) }}\n\
             fn mv() -> Result[Str, Vec[Diag]] {{ let n = lv()?; Result[Str, Vec[Diag]].Ok(value: int_to_str(n)) }}\n\
             fn ts() -> Result[Str, Str] {{ let n = ms()?; Result[Str, Str].Ok(value: int_to_str(n)) }}\n\
             fn tr() -> Result[Int, Item] {{ let n = mr()?; Result[Int, Item].Ok(value: n) }}\n\
             fn tv() -> Result[Int, Vec[Diag]] {{ let s = mv()?; Result[Int, Vec[Diag]].Ok(value: str_len(s)) }}\n\
             fn text(n: Int) -> Result[Str, Str] {{ Result[Str, Str].Ok(value: str_concat(\"value-\", int_to_str(n))) }}\n\
             fn ok() -> Result[Int, Str] {{ let s = text(12345)?; churn(); Result[Int, Str].Ok(value: str_len(s)) }}\n\
             fn main() -> Int {{\n    let a = ts(); churn(); let b = tr(); churn(); let c = tv(); churn();\n    \
             match a {{ Result.Ok(value: s) => 0, Result.Err(error: e) => str_len(e) * 10000, }}\n        \
             + match b {{ Result.Ok(value: n) => 0, Result.Err(error: i) => str_len(i.name) * 100 + ints_get(i.tags, 0), }}\n        \
             + match c {{ Result.Ok(value: n) => 0, Result.Err(error: ds) => vec_len(ds) * 1000000, }}\n        \
             + match ok() {{ Result.Ok(value: n) => n * 100000000, Result.Err(error: e) => 0, }}\n}}\n"),
         "1105090731"),
        // A half-evaluated call, and a half-built record and variant, with `?` in the middle.
        (format!("{churn}struct Three {{ a: Str, b: Int, c: Ints, }}\nenum Held {{ Three(a: Str, b: Int, c: Ints), Nothing, }}\n\
             fn may_fail(n: Int) -> Result[Int, Str] {{ if n % 2 == 0 {{ return Result[Int, Str].Err(error: int_to_str(n)); }} Result[Int, Str].Ok(value: n) }}\n\
             fn later(log: Ints) -> Str {{ ints_push(log, 1); str_concat(\"late\", \"r\") }}\n\
             fn consume(log: Ints, a: Str, b: Int, c: Str) -> Int {{ ints_push(log, 2); str_len(a) + b + str_len(c) }}\n\
             fn call(log: Ints, i: Int) -> Result[Int, Str] {{ Result[Int, Str].Ok(value: consume(log, int_to_str(i * 1000), may_fail(i)?, later(log))) }}\n\
             fn rec(n: Int) -> Result[Three, Str] {{ Result[Three, Str].Ok(value: Three(a: int_to_str(n * 11), b: may_fail(n)?, c: ints_new())) }}\n\
             fn var(n: Int) -> Result[Held, Str] {{ Result[Held, Str].Ok(value: Held.Three(a: int_to_str(n * 11), b: may_fail(n)?, c: ints_new())) }}\n\
             fn main() -> Int {{\n    let log = ints_new(); let mut i = 0; let mut t = 0;\n    \
             while i < 8 {{\n        t = t + match call(log, i) {{ Result.Ok(value: n) => n, Result.Err(error: e) => 0 - str_len(e), }};\n        \
             t = t + match rec(i) {{ Result.Ok(value: r) => str_len(r.a) + r.b, Result.Err(error: e) => 100, }};\n        \
             t = t + match var(i) {{ Result.Ok(value: h) => match h {{ Held.Three(a: a, b: b, c: c) => str_len(a) + b, Held.Nothing() => 0, }}, Result.Err(error: e) => 1000, }};\n        \
             i = i + 1;\n    }}\n    churn();\n    t * 100 + ints_len(log)\n}}\n"),
         "449608"),
        // A scope whose task is still running when `?` leaves it.
        ("fn worker(out: Chan, tag: Str) -> Int { let mut i = 0; let mut s = 0; while i < 200000 { s = s + i % 3; i = i + 1; } chan_send(out, str_len(tag)); s }\n\
          fn fails() -> Result[Int, Str] { Result[Int, Str].Err(error: int_to_str(9)) }\n\
          fn run(out: Chan, tasks: SpawnCap) -> Result[Int, Str] { let tag = str_concat(\"task-\", int_to_str(1)); scope { spawn worker(out, tag); let held = int_to_str(555); let x = fails()?; chan_send(out, x + str_len(held)); } Result[Int, Str].Ok(value: 0) }\n\
          fn main(tasks: SpawnCap) -> Int { let out = chan_new(4); let r = run(out, tasks); chan_close(out); let buf = ints_new(); let got = chan_recv(out, buf); \
          match r { Result.Ok(value: n) => 0, Result.Err(error: e) => str_len(e) * 1000, } + ints_len(buf) * 100 + ints_get(buf, 0) }\n".to_owned(),
         "1106"),
        // A generic function's instances, and one `match` on a propagated value.
        (format!("{churn}enum Shape {{ Named(label: Str), Plain, }}\n\
             fn pass[T, E](r: Result[T, E]) -> Result[T, E] {{ let v = r?; Result[T, E].Ok(value: v) }}\n\
             fn make(n: Int) -> Result[Shape, Str] {{ if n == 0 {{ return Result[Shape, Str].Err(error: int_to_str(12345)); }} Result[Shape, Str].Ok(value: Shape.Named(label: str_concat(\"shape-\", int_to_str(n)))) }}\n\
             fn f(n: Int) -> Result[Int, Str] {{ let k = match make(n)? {{ Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }}; churn(); Result[Int, Str].Ok(value: k) }}\n\
             fn main() -> Int {{\n    let a = match pass(Result[Int, Str].Err(error: int_to_str(123))) {{ Result.Ok(value: n) => n, Result.Err(error: s) => str_len(s), }};\n    \
             let b = match pass(Result[Str, Int].Ok(value: str_concat(\"ab\", \"cd\"))) {{ Result.Ok(value: s) => str_len(s), Result.Err(error: n) => n, }};\n    \
             let c = match f(7) {{ Result.Ok(value: k) => k, Result.Err(error: e) => 0, }};\n    \
             let d = match f(0) {{ Result.Ok(value: k) => 0, Result.Err(error: e) => str_len(e), }};\n    \
             a * 1000 + b * 100 + c * 10 + d\n}}\n"),
         "3475"),
    ];
    let borrowed: Vec<(&str, &str)> = cases.iter().map(|(s, e)| (s.as_str(), *e)).collect();
    same_memory_semantics(dir.path(), &emitter, &borrowed);
}

const BLOCK_COMPARABLE: &[&str] = &[
    "N0200", "N0300", "N0302", "N0313", "N0343", "N0360", "N0361", "N0362",
];

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_value_blocks() {
    // N12.1: a block used as a value, in every position the grammar allows one, and the
    // completion rules it is checked by — `Value`, `Unit` and `Diverges`, folded across
    // arms exactly as the reference folds them. Both checkers must agree on every accepted
    // program and on every refusal's code and span.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let sources = [
        // Accepted.
        "fn f(o: Option[Int]) -> Int { match o { Option.None() => { let n = 1; n + 1 }, Option.Some(value: v) => { let y = v * 10; y }, } }\nfn main() -> Int { 0 }",
        "fn main() -> Int { let x = { let a = 2; a * 3 }; x }",
        "fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { add({ let a = 1; a + 1 }, 5) + { 3 } }",
        "struct P { a: Int, s: Str, }\nfn main() -> Int { let p = P(a: { let z = 4; z }, s: { let t = int_to_str(9); t }); p.a }",
        "fn main() -> Int { let o = Option[Str].Some(value: { let t = int_to_str(12); t }); match o { Option.None() => 0, Option.Some(value: v) => str_len(v), } }",
        "fn f() -> Int { return { let q = 5; q + 1 }; }\nfn main() -> Int { f() }",
        "fn choose[T](x: Option[T], fallback: T) -> T { match x { Option.None() => { fallback }, Option.Some(value: v) => { v }, } }\nfn main() -> Int { choose(Option[Int].None(), 3) }",
        "fn g(ok: Bool) -> Result[Int, Str] { Result[Int, Str].Ok(value: 1) }\nenum S { A, B, }\nfn f(s: S) -> Result[Int, Str] { let x = match s { S.A() => { let n = g(true)?; n + 1 }, S.B() => { 0 }, }; Result[Int, Str].Ok(value: x) }\nfn main() -> Int { 0 }",
        "fn g() -> Result[Int, Str] { Result[Int, Str].Ok(value: 1) }\nfn f(c: Bool) -> Result[Int, Str] { let a = (if c { g() } else { g() })?; let b = ({ let r = g(); r })?; Result[Int, Str].Ok(value: a + b) }\nfn main() -> Int { 0 }",
        "fn main() -> Int { let mut x = 1; { let y = 2; x = x + y; } x }",
        "fn main() -> Int { let mut x = 0; match Option[Int].Some(value: 5) { Option.None() => { x = 1; }, Option.Some(value: v) => { x = v; }, }; x }",
        "fn f(o: Option[Int]) -> Int { let x = match o { Option.None() => { return 7; }, Option.Some(value: v) => { return v; }, }; x }\nfn main() -> Int { 0 }",
        "enum E { A, B, C, }\nfn f(e: E) -> Int { match e { E.A() => { return 1; }, E.B() => { let s = int_to_str(3); str_len(s) }, E.C() => { 9 }, } }\nfn main() -> Int { 0 }",
        "fn main() -> Int { let mut i = 0; while i < 5 { i = i + 1; let v = match Option[Int].Some(value: i) { Option.None() => { continue; }, Option.Some(value: n) => { if n == 3 { break; } n }, }; } i }",
        // Refused: completions that do not join.
        "fn f(c: Bool) -> Int { let mut z = 0; if c { 1 } else { z = 2; } }\nfn main() -> Int { 0 }",
        "fn f(o: Option[Int]) -> Int { let mut z = 0; match o { Option.None() => { z = 1; }, Option.Some(value: v) => { v }, } }\nfn main() -> Int { 0 }",
        "fn f(o: Option[Int]) -> Int { let mut z = 0; let x = match o { Option.None() => { z = 1; }, Option.Some(value: v) => { v }, }; x }\nfn main() -> Int { 0 }",
        "fn f(o: Option[Int]) -> Int { match o { Option.None() => { 1 }, Option.Some(value: v) => { let s = int_to_str(v); s }, } }\nfn main() -> Int { 0 }",
        "fn main() -> Int { let mut z = 0; let x = { z = 1; }; 0 }",
        "fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { let mut z = 0; add({ z = 1; }, 2) }",
        "fn main() -> Int { let x = { let s = int_to_str(1); s }; x }",
        // Refused: a tail after a transfer, and a block that cannot reach the value it names.
        "fn main() -> Int { let x = { return 1; 2 }; x }",
        "fn main() -> Int { let x = { let a = 1; a }; a }",
        // Refused: the rules around it still apply inside a block.
        "fn f(o: Option[Int]) -> Int { match o { Option.None() => { 0 }, } }\nfn main() -> Int { 0 }",
        "fn f(o: Option[Int]) -> Result[Int, Str] { let x = match o { Option.None() => { 0 }, Option.Some(value: v) => { o? }, }; Result[Int, Str].Ok(value: x) }\nfn main() -> Int { 0 }",
        "fn f(r: Result[Int, Str]) -> Int { let x = { let n = r?; n }; x }\nfn main() -> Int { 0 }",
    ];

    let mut refusals = 0;
    for source in &sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout)
            .into_iter()
            .filter(|(c, _, _)| BLOCK_COMPARABLE.contains(&c.as_str()))
            .collect();
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .filter(|d| BLOCK_COMPARABLE.contains(&d.code.as_str()))
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        if theirs.is_empty() {
            assert_eq!(
                status,
                Some(0),
                "{source:?} should have been accepted: {stdout}"
            );
        } else {
            refusals += 1;
            assert_eq!(status, Some(1), "{source:?} should have been refused");
        }
    }
    assert!(refusals >= 10, "only {refusals} refusals were compared");
}

#[test]
fn the_nazm_written_compiler_compiles_value_blocks_with_the_same_memory_semantics() {
    // N12.1: block values compiled by the Nazm-written compiler, against the reference, on
    // the shapes where a retain placed after a release shows — a `Str` tail outliving its
    // local, payloads handed back through arm blocks, a `?` inside a `scope` inside an arm,
    // an arm that returns beside one that propagates, a transfer out of a block argument
    // and out of a block field initialiser, and a statement `match` whose arms are `Unit`.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let prelude = "fn churn() -> Int { let mut noise = vec_new[Str](); let mut k = 0; \
                   while k < 64 { vec_push(noise, str_concat(\"noise-\", int_to_str(k))); k = k + 1; } \
                   noise = vec_new[Str](); vec_len(noise) }\n\
                   struct Item { name: Str, tags: Ints, }\nenum Shape { Named(label: Str), Plain, }\n\
                   fn item(n: Int) -> Item { let t = ints_new(); ints_push(t, n); Item(name: str_concat(\"item-\", int_to_str(n)), tags: t) }\n";
    let cases: Vec<(String, &str)> = vec![
        (format!("{prelude}fn make(n: Int) -> Str {{ str_concat(\"text-\", int_to_str(n)) }}\n\
             fn main() -> Int {{ let mut total = 0; let mut i = 0; while i < 5 {{ let s = {{ let t = make(i * 111); t }}; churn(); \
             total = total + str_len(s); i = i + 1; }} let it = {{ let a = item(7); let b = item(8); a }}; \
             let sh = {{ let l = str_concat(\"lab\", int_to_str(1234)); let made = Shape.Named(label: l); made }}; churn(); \
             total * 10000 + str_len(it.name) * 100 + ints_get(it.tags, 0) + match sh {{ Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }} * 1000000 }}\n"),
         "7380607"),
        (format!("{prelude}enum Bag {{ Full(items: Vec[Str]), Empty, }}\n\
             fn text(o: Option[Str]) -> Str {{ match o {{ Option.None() => {{ let d = str_concat(\"no\", \"ne\"); d }}, Option.Some(value: s) => {{ let x = s; x }}, }} }}\n\
             fn thing(r: Result[Item, Str]) -> Item {{ match r {{ Result.Ok(value: i) => {{ let kept = i; kept }}, Result.Err(error: e) => {{ let fb = item(str_len(e)); fb }}, }} }}\n\
             fn bag(b: Bag) -> Vec[Str] {{ match b {{ Bag.Full(items: xs) => {{ let ys = xs; ys }}, Bag.Empty() => {{ let fresh = vec_new[Str](); vec_push(fresh, int_to_str(0)); fresh }}, }} }}\n\
             fn choose[T](x: Option[T], fallback: T) -> T {{ match x {{ Option.None() => {{ let f = fallback; f }}, Option.Some(value: v) => {{ let w = v; w }}, }} }}\n\
             fn main() -> Int {{ let a = text(Option[Str].Some(value: str_concat(\"some-\", int_to_str(42)))); let b = text(Option[Str].None()); churn(); \
             let i = thing(Result[Item, Str].Ok(value: item(3))); let j = thing(Result[Item, Str].Err(error: int_to_str(12345))); churn(); \
             let full = vec_new[Str](); vec_push(full, int_to_str(777)); vec_push(full, int_to_str(8)); \
             let c = bag(Bag.Full(items: full)); let d = bag(Bag.Empty()); let g = choose(Option[Str].None(), int_to_str(1234)); churn(); \
             str_len(a) * 1000000 + str_len(b) * 100000 + ints_get(i.tags, 0) * 10000 + ints_get(j.tags, 0) * 1000 \
             + vec_len(c) * 100 + str_len(vec_get(c, 0)) * 10 + vec_len(d) + str_len(g) * 10000000 }}\n"),
         "47435231"),
        ("fn worker(out: Chan, tag: Str) -> Int { let mut i = 0; let mut s = 0; while i < 200000 { s = s + i % 3; i = i + 1; } chan_send(out, str_len(tag)); s }\n\
          fn g(ok: Bool) -> Result[Int, Str] { if ok { return Result[Int, Str].Ok(value: 41); } Result[Int, Str].Err(error: int_to_str(404)) }\n\
          enum State { A, B, }\n\
          fn f(s: State, ok: Bool, out: Chan, tasks: SpawnCap) -> Result[Int, Str] { let x = match s { State.A() => { let held = str_concat(\"held-\", int_to_str(1)); let mut n = 0; \
          scope { spawn worker(out, held); let inner = int_to_str(99); n = g(ok)? + str_len(inner); } n + str_len(held) }, State.B() => { 0 }, }; Result[Int, Str].Ok(value: x) }\n\
          fn main(tasks: SpawnCap) -> Int { let out = chan_new(8); let a = f(State.A(), true, out, tasks); let b = f(State.A(), false, out, tasks); let c = f(State.B(), false, out, tasks); chan_close(out); \
          let buf = ints_new(); let got = chan_recv(out, buf); let more = chan_recv(out, buf); let rest = chan_recv(out, buf); \
          let va = match a { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, }; let vb = match b { Result.Ok(value: v) => v, Result.Err(error: e) => str_len(e), }; \
          let vc = match c { Result.Ok(value: v) => v, Result.Err(error: e) => 0 - 1, }; va * 10000 + vb * 1000 + vc * 100 + ints_len(buf) * 10 + ints_get(buf, 0) }\n".to_owned(),
         "493026"),
        (format!("{prelude}enum E {{ A, B, C, }}\n\
             fn g(ok: Bool) -> Result[Int, Str] {{ if ok {{ return Result[Int, Str].Ok(value: 5); }} Result[Int, Str].Err(error: str_concat(\"x\", \"y\")) }}\n\
             fn f(e: E, ok: Bool) -> Result[Int, Str] {{ let v = match e {{ E.A() => {{ let gone = int_to_str(1); return Result[Int, Str].Ok(value: 100); }}, \
             E.B() => {{ let s = str_concat(\"ab\", \"c\"); str_len(s) }}, E.C() => {{ let held = int_to_str(22); g(ok)? + str_len(held) }}, }}; Result[Int, Str].Ok(value: v) }}\n\
             fn show(r: Result[Int, Str]) -> Int {{ match r {{ Result.Ok(value: v) => v, Result.Err(error: e) => 0 - str_len(e), }} }}\n\
             fn main() -> Int {{ let mut i = 0; let mut loops = 0; while i < 10 {{ i = i + 1; let o = if i % 2 == 0 {{ Option[Str].Some(value: int_to_str(i * 100)) }} else {{ Option[Str].None() }}; \
             let v = match o {{ Option.None() => {{ let w = str_concat(\"skip-\", int_to_str(i)); continue; }}, Option.Some(value: s) => {{ let k = s; if i == 8 {{ break; }} k }}, }}; loops = loops + str_len(v); }} \
             churn(); show(f(E.A(), true)) * 10000 + show(f(E.B(), true)) * 1000 + show(f(E.C(), true)) * 10 + show(f(E.C(), false)) + loops * 10000000 }}\n"),
         "91003068"),
        (format!("{prelude}fn later(log: Ints) -> Str {{ ints_push(log, 1); str_concat(\"late\", \"r\") }}\n\
             fn consume(a: Str, b: Str, c: Str) -> Int {{ str_len(a) + str_len(b) + str_len(c) }}\n\
             fn go(fail: Bool, log: Ints) -> Int {{ consume(int_to_str(12345), {{ let t = str_concat(\"mid-\", int_to_str(1)); if fail {{ return 0 - 1; }} t }}, later(log)) }}\n\
             fn record(fail: Bool) -> Int {{ let it = Item(name: str_concat(\"nm-\", int_to_str(7)), tags: {{ let ts = ints_new(); ints_push(ts, 3); if fail {{ return 0 - 1; }} ts }}); \
             str_len(it.name) * 10 + ints_get(it.tags, 0) }}\n\
             fn variant(fail: Bool) -> Int {{ let sh = Shape.Named(label: {{ let l = int_to_str(4444); if fail {{ return 0 - 1; }} l }}); \
             match sh {{ Shape.Named(label: l) => str_len(l), Shape.Plain() => 0, }} }}\n\
             fn main() -> Int {{ let log = ints_new(); let a = go(false, log); let b = go(true, log); let r = record(false); let q = record(true); \
             let c = variant(false); let d = variant(true); churn(); \
             a * 100000000 + (b + 1) * 10000000 + ints_len(log) * 1000000 + r * 1000 + (q + 1) * 100 + c * 10 + (d + 1) }}\n"),
         "1501043040"),
        (format!("{prelude}fn main() -> Int {{ let mut name = str_concat(\"start\", \"\"); let mut i = 0; while i < 6 {{ \
             let o = if i % 3 == 0 {{ Option[Str].None() }} else {{ Option[Str].Some(value: int_to_str(i * 1000)) }}; \
             match o {{ Option.None() => {{ name = str_concat(name, \".\"); }}, Option.Some(value: s) => {{ let t = s; name = t; }}, }}; i = i + 1; }} \
             {{ let tail = str_concat(name, \"\"); name = tail; }} churn(); str_len(name) }}\n"),
         "4"),
    ];
    let borrowed: Vec<(&str, &str)> = cases.iter().map(|(s, e)| (s.as_str(), *e)).collect();
    same_memory_semantics(dir.path(), &emitter, &borrowed);
}

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_equality() {
    // N13, `docs/spec.md`, *Equality is derived*. Both checkers derive `==` from the same
    // table — a record when every field has it, an enum when every payload field of every
    // variant does, an instance after substitution — and must agree on every accepted
    // program and on every refusal's code and span.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let sources = [
        // Accepted.
        "struct Point { x: Int, y: Int, }\nfn main() -> Int { if Point(x: 1, y: 2) == Point(x: 1, y: 2) { 1 } else { 0 } }",
        "struct N { name: Str, n: Int, flag: Bool, }\nfn f(a: N, b: N) -> Bool { a != b }\nfn main() -> Int { 0 }",
        "enum E { A, B(v: Int), C(s: Str, t: Str), }\nfn f(a: E, b: E) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "enum Solo { Only, }\nfn main() -> Int { if Solo.Only() == Solo.Only() { 1 } else { 0 } }",
        "fn main() -> Int { if Option[Int].None() == Option[Int].Some(value: 1) { 1 } else { 0 } }",
        "fn f(a: Result[Int, Str], b: Result[Int, Str]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct Box[T] { value: T, }\nstruct P { x: Int, }\nfn f(a: Box[P], b: Box[P]) -> Bool { a == b }\nfn main() -> Int { if Box[Str](value: \"a\") != Box[Str](value: \"b\") { 1 } else { 0 } }",
        "struct Name { t: Str, }\nenum Shape { Dot, Circle(c: Name, r: Int), }\nstruct D { s: Shape, }\nenum L { H, S(top: D), }\nfn f(a: L, b: L) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct Box[T] { value: T, tag: Int, }\nfn same_tag[T](a: Box[T], b: Box[T]) -> Bool { a.tag == b.tag }\nfn main() -> Int { 0 }",
        "enum Shape { Named(label: Str), Plain, }\nfn f(s: Shape) -> Bool { match s { Shape.Named(label: x) => { let w = str_concat(\"a\", \"b\"); x == w }, Shape.Plain() => { false }, } }\nfn main() -> Int { 0 }",
        // Refused: one type, never two.
        "struct A { x: Int, }\nstruct B { x: Int, }\nfn main() -> Int { if A(x: 1) == B(x: 1) { 1 } else { 0 } }",
        "struct Box[T] { value: T, }\nfn main() -> Int { if Box[Int](value: 1) == Box[Str](value: \"1\") { 1 } else { 0 } }",
        "fn main() -> Int { if Option[Int].None() == Option[Str].None() { 1 } else { 0 } }",
        // Refused: a component without equality, however deep and in whichever variant.
        "struct B { n: Int, values: Vec[Int], }\nfn f(a: B, b: B) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct B { values: Ints, }\nstruct D { n: Int, inner: B, }\nfn f(a: D, b: D) -> Bool { a != b }\nfn main() -> Int { 0 }",
        "struct C { ch: Chan, }\nfn f(a: C, b: C) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "enum Work { Empty, Values(value: Vec[Int]), Count(n: Int), }\nfn main() -> Int { if Work.Empty() == Work.Empty() { 1 } else { 0 } }",
        "enum M { A(n: Int), B(c: Chan), C(s: Str), }\nfn main() -> Int { if M.A(n: 1) == M.A(n: 1) { 1 } else { 0 } }",
        "enum L { A(n: Int), B(s: Str), C(xs: Strs), }\nfn main() -> Int { if L.A(n: 1) != L.A(n: 1) { 1 } else { 0 } }",
        "struct Leaf { xs: Vec[Int], }\nenum Mid { N, Has(leaf: Leaf), }\nstruct Top { mid: Mid, }\nfn f(a: Top, b: Top) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "enum E { A, B(maybe: Option[Vec[Int]]), }\nfn main() -> Int { if E.A() == E.A() { 1 } else { 0 } }",
        "fn main() -> Int { if Option[Vec[Int]].None() == Option[Vec[Int]].None() { 1 } else { 0 } }",
        "struct Diag { code: Str, }\nfn f(a: Result[Int, Vec[Diag]], b: Result[Int, Vec[Diag]]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct Box[T] { value: T, }\nfn f(a: Box[Box[Chan]], b: Box[Box[Chan]]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct Pair[A, B] { first: A, second: B, }\nfn f(a: Pair[Strs, Int], b: Pair[Strs, Int]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        // Refused: an unconstrained parameter, bare or applied.
        "fn same[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "struct Box[T] { value: T, }\nfn same_box[T](a: Box[T], b: Box[T]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "fn same[T](a: Option[T], b: Option[T]) -> Bool { a != b }\nfn main() -> Int { 0 }",
        // Refused: handles, and a record ordered against a number (ordering two records is the
        // 1.0 draft's since R1a, L6, and outside this compiler's subset until R2).
        "fn main() -> Int { let a = vec_new[Int](); if a == a { 1 } else { 0 } }",
        "struct Point { x: Int, }\nfn main() -> Int { if Point(x: 1) < 2 { 1 } else { 0 } }",
    ];

    let mut refusals = 0;
    for source in &sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout);
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        if theirs.is_empty() {
            assert_eq!(
                status,
                Some(0),
                "{source:?} should have been accepted: {stdout}"
            );
        } else {
            refusals += 1;
            assert_eq!(status, Some(1), "{source:?} should have been refused");
        }
    }
    assert!(refusals >= 18, "only {refusals} refusals were compared");
}

#[test]
fn the_nazm_written_compiler_compiles_equality_with_the_same_memory_semantics() {
    // N13: records, enums, `Option`, `Result` and generic instances compared by the
    // Nazm-written compiler, against the reference, on the stated answers — with owning
    // temporaries on both sides, bound operands read again afterwards, a different-variant
    // comparison that must read no payload, and churn after each to make a premature
    // release show.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let prelude = "fn churn() -> Int { let mut noise = vec_new[Str](); let mut k = 0; \
                   while k < 64 { vec_push(noise, str_concat(\"noise-\", int_to_str(k))); k = k + 1; } \
                   noise = vec_new[Str](); vec_len(noise) }\n\
                   fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
                   fn text(n: Int) -> Str { str_concat(\"t-\", int_to_str(n)) }\n\
                   struct Item { name: Str, n: Int, }\n\
                   enum Shape { Plain, Named(label: Str), Pair(left: Str, right: Str), }\n\
                   struct Outer { item: Item, shape: Shape, note: Option[Str], }\n\
                   struct Box[T] { value: T, }\n\
                   fn item(n: Int) -> Item { Item(name: text(n), n: n) }\n\
                   fn shape(k: Int) -> Shape { if k % 3 == 0 { return Shape.Plain(); } if k % 3 == 1 { return Shape.Named(label: text(k)); } Shape.Pair(left: text(k), right: text(k + 1)) }\n\
                   fn outer(n: Int) -> Outer { Outer(item: item(n), shape: shape(n), note: Option[Str].Some(value: text(n))) }\n";
    let cases: Vec<(String, &str)> = vec![
        (format!("{prelude}struct Point {{ x: Int, y: Int, }}\n\
             fn main() -> Int {{ let p = Point(x: 1, y: 2); \
             bit(p == Point(x: 1, y: 2), 1) + bit(p == Point(x: 9, y: 2), 2) + bit(p == Point(x: 1, y: 9), 4) + bit(p != Point(x: 1, y: 9), 8) + bit(p != p, 16) \
             + bit(Item(name: str_concat(\"a\", \"b\"), n: 2) == Item(n: 2, name: \"ab\"), 32) + bit(Item(name: \"ab\", n: 2) == Item(name: \"ab\", n: 3), 64) }}\n"),
         "41"),
        (format!("{prelude}enum MaybeInt {{ None, Some(value: Int), }}\nenum Twins {{ Left(n: Int), Right(n: Int), }}\nenum Solo {{ Only, }}\n\
             fn main() -> Int {{ bit(MaybeInt.None() == MaybeInt.None(), 1) + bit(MaybeInt.None() == MaybeInt.Some(value: 0), 2) \
             + bit(MaybeInt.Some(value: 1) == MaybeInt.Some(value: 1), 4) + bit(MaybeInt.Some(value: 1) == MaybeInt.Some(value: 2), 8) \
             + bit(Twins.Left(n: 1) == Twins.Right(n: 1), 16) + bit(Twins.Right(n: 1) != Twins.Right(n: 2), 32) \
             + bit(Shape.Named(label: text(1)) == Shape.Plain(), 64) + bit(Shape.Pair(left: text(1), right: text(2)) == Shape.Pair(right: text(2), left: text(1)), 128) \
             + bit(Solo.Only() == Solo.Only(), 256) }}\n"),
         "421"),
        (format!("{prelude}struct Point {{ x: Int, y: Int, }}\n\
             fn main() -> Int {{ bit(Option[Int].None() == Option[Int].None(), 1) + bit(Option[Str].Some(value: text(1)) == Option[Str].Some(value: \"t-1\"), 2) \
             + bit(Result[Int, Str].Ok(value: 1) == Result[Int, Str].Err(error: \"1\"), 4) + bit(Result[Str, Str].Err(error: text(3)) == Result[Str, Str].Err(error: text(3)), 8) \
             + bit(Box[Int](value: 1) == Box[Int](value: 1), 16) + bit(Box[Str](value: text(1)) == Box[Str](value: text(2)), 32) \
             + bit(Box[Point](value: Point(x: 1, y: 2)) == Box[Point](value: Point(x: 1, y: 2)), 64) + bit(Box[Box[Int]](value: Box[Int](value: 4)) != Box[Box[Int]](value: Box[Int](value: 5)), 128) }}\n"),
         "219"),
        (format!("{prelude}fn main() -> Int {{ let mut total = 0; let mut i = 0; while i < 6 {{ \
             total = total + bit(item(i) == item(i), 1) + bit(item(i) != item(i + 1), 10); \
             total = total + bit(shape(i) == shape(i), 100) + bit(shape(i) == shape(i + 1), 1000); \
             total = total + bit(outer(i) == outer(i), 10000); \
             total = total + bit(Option[Str].Some(value: text(i)) == Option[Str].Some(value: text(i)), 100000); \
             total = total + bit(Result[Str, Str].Ok(value: text(i)) == Result[Str, Str].Err(error: text(i)), 1000000); \
             churn(); i = i + 1; }} total }}\n"),
         "660666"),
        (format!("{prelude}fn main() -> Int {{ let a = outer(4); let b = outer(4); let c = outer(5); \
             let same = a == b; let differ = a != c; churn(); let mut n = 0; if same {{ n = n + 1; }} if differ {{ n = n + 10; }} \
             n + 100 * (str_len(a.item.name) + str_len(b.item.name) + str_len(c.item.name)) }}\n"),
         "911"),
        (format!("{prelude}fn right(k: Int) -> Int {{ if item(k) == Item(name: text(k), n: {{ return 7; }}) {{ 1 }} else {{ 2 }} }}\n\
             fn found(ok: Bool) -> Result[Option[Str], Int] {{ if ok {{ Result[Option[Str], Int].Ok(value: Option[Str].Some(value: text(3))) }} else {{ Result[Option[Str], Int].Err(error: 3) }} }}\n\
             fn compare(ok: Bool) -> Result[Int, Int] {{ let got = found(ok)?; if got == Option[Str].Some(value: text(3)) {{ Result[Int, Int].Ok(value: 1) }} else {{ Result[Int, Int].Ok(value: 0) }} }}\n\
             fn main() -> Int {{ let s = shape(1); let same = match s {{ Shape.Named(label: x) => {{ let want = text(1); x == want }}, Shape.Plain() => {{ false }}, Shape.Pair(left: _, right: _) => {{ false }}, }}; \
             churn(); let one = if same {{ 1 }} else {{ 0 }}; \
             let ten = match compare(true) {{ Result.Ok(value: v) => 10 * v, Result.Err(error: _) => 0, }}; \
             let hundred = match compare(false) {{ Result.Ok(value: _) => 0, Result.Err(error: c) => 100 * c, }}; \
             one + ten + hundred + right(1) * 1000 }}\n"),
         "7311"),
    ];
    let borrowed: Vec<(&str, &str)> = cases.iter().map(|(s, e)| (s.as_str(), *e)).collect();
    same_memory_semantics(dir.path(), &emitter, &borrowed);
}

#[test]
fn the_nazm_checker_agrees_with_the_reference_on_requirements() {
    // N14, `docs/spec.md`, *A type parameter may require equality*. Both checkers record
    // the same requirement on the same position, give the body `==` by it, check every
    // argument against it by N13's derivation, and refuse what they refuse with the same
    // code at the same span.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let same = "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n";
    let sources: Vec<String> = [
        // Accepted.
        "fn main() -> Int { if same(1, 1) { 1 } else { 0 } }",
        "fn main() -> Int { if same(\"a\", \"b\") { 1 } else { 0 } }",
        "struct Point { x: Int, y: Int, }\nfn main() -> Int { if same(Point(x: 1, y: 2), Point(x: 1, y: 2)) { 1 } else { 0 } }",
        "fn main() -> Int { if same(Option[Int].Some(value: 1), Option[Int].None()) { 1 } else { 0 } }",
        "fn main() -> Int { if same[Result[Int, Str]](Result[Int, Str].Ok(value: 1), Result[Int, Str].Ok(value: 1)) { 1 } else { 0 } }",
        "struct Box[T] { value: T, }\nfn box_same[T: Equality](a: Box[T], b: Box[T]) -> Bool { a == b }\nfn main() -> Int { if box_same(Box[Int](value: 1), Box[Int](value: 1)) { 1 } else { 0 } }",
        "fn wrap[T: Equality](a: T, b: T) -> Bool { same(a, b) }\nfn main() -> Int { if wrap(1, 1) { 1 } else { 0 } }",
        "fn res[T: Equality, E: Equality](a: Result[T, E], b: Result[T, E]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "fn pick[A: Equality, B](a: A, b: B) -> Int { 0 }\nfn main() -> Int { pick(1, vec_new[Int]()) }",
        "struct Equality { x: Int, }\nfn main() -> Int { Equality(x: 1).x }",
        // Refused: an argument without equality, inferred or written.
        "fn main() -> Int { let v = vec_new[Int](); if same(v, v) { 1 } else { 0 } }",
        "fn main() -> Int { let c = chan_new(1); if same(c, c) { 1 } else { 0 } }",
        "fn main() -> Int { let o = Option[Vec[Int]].None(); if same(o, o) { 1 } else { 0 } }",
        "struct Box[T] { value: T, }\nfn main() -> Int { let a = Box[Vec[Int]](value: vec_new[Int]()); if same[Box[Vec[Int]]](a, a) { 1 } else { 0 } }",
        "enum Work { Empty, Values(value: Vec[Int]), }\nfn main() -> Int { if same(Work.Empty(), Work.Empty()) { 1 } else { 0 } }",
        "fn both[A: Equality, B: Equality](a: A, b: B) -> Int { 0 }\nfn main() -> Int { both(1, vec_new[Int]()) }",
        // Refused: the body, and forwarding.
        "fn other[T](a: T, b: T) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "fn wrap[T](a: T, b: T) -> Bool { same(a, b) }\nfn main() -> Int { 0 }",
        "fn f[T: Equality, E](a: Result[T, E], b: Result[T, E]) -> Bool { a == b }\nfn main() -> Int { 0 }",
        "fn wrong[A: Equality, B: Equality](a: A, b: B) -> Bool { a == b }\nfn main() -> Int { 0 }",
        // Refused: what may be written.
        "fn f[T: Ord](a: T) -> Int { 0 }\nfn main() -> Int { 0 }",
        "struct B[T: Equality] { v: T, }\nfn main() -> Int { 0 }",
        "enum E[T: Equality] { A(v: T), }\nfn main() -> Int { 0 }",
    ]
    .iter()
    .map(|s| format!("{same}{s}"))
    .collect();

    let mut refusals = 0;
    for source in &sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout);
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        if theirs.is_empty() {
            assert_eq!(
                status,
                Some(0),
                "{source:?} should have been accepted: {stdout}"
            );
        } else {
            refusals += 1;
            assert_eq!(status, Some(1), "{source:?} should have been refused");
        }
    }
    assert!(refusals >= 13, "only {refusals} refusals were compared");
}

/// N102: the features ported to close the parity record — effect sets, capabilities,
/// recursion, contracts, foreign declarations, standard modules, traits, closures and the
/// logical operators — checked by both checkers, with the same code at the same span, on what
/// each accepts and refuses.
#[test]
fn the_nazm_checker_agrees_with_the_reference_on_the_n102_features() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_checker(dir.path());
    let sources: &[&str] = &[
        // Effect sets: accepted, and every one of their three refusals.
        "fn say(io: IoCap) -> Int ! { io } { print(\"x\"); 0 }\nfn main(io: IoCap) -> Int ! { io } { say(io) }",
        "fn quiet() -> Int ! {} { 1 }\nfn main() -> Int ! {} { quiet() }",
        "fn say() -> Int { print(\"x\"); 0 }\nfn f(io: IoCap) -> Int ! {} { say() }\nfn main(io: IoCap) -> Int ! { io } { f(io) }",
        "fn main(io: IoCap) -> Int ! { io, io } { 0 }",
        "fn main(io: IoCap) -> Int ! { wat } { 0 }",
        "fn w(n: Int) -> Int { n }\nfn main(sp: SpawnCap) -> Int ! {} { scope { spawn w(1); } 0 }",
        // Capabilities: a root, an attenuation, and a parameter of `main` that is none.
        "fn out(o: OutCap) -> Int ! { io } { print(\"x\"); 0 }\nfn main(io: IoCap) -> Int ! { io } { out(io) }",
        "fn main(n: Int) -> Int { n }",
        // Contracts: a clause that is no `Bool`, and three it may not contain.
        "fn f(n: Int) -> Int ! {}\n    requires n >= 0\n    ensures result >= n\n{ n }\nfn main() -> Int { f(1) }",
        "fn f(n: Int) -> Int ! {}\n    requires n\n{ n }\nfn main() -> Int { 0 }",
        "fn g() -> Int { 1 }\nfn f(n: Int) -> Int ! {}\n    requires g() > 0\n{ n }\nfn main() -> Int { 0 }",
        "struct P { x: Int }\nfn f(n: Int) -> Int ! {}\n    requires P(x: n).x > 0\n{ n }\nfn main() -> Int { 0 }",
        "fn f(n: Int) -> Int ! {}\n    requires if n > 0 { true } else { false }\n{ n }\nfn main() -> Int { 0 }",
        // Foreign declarations.
        "extern \"C\" fn labs(x: Int) -> Int = \"labs\";\nfn main(f: ForeignCap) -> Int ! { foreign } { labs(0 - 3) }",
        // Traits: accepted, and each refusal the reference makes.
        "trait Area { fn area(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Int { self.s * self.s } }\nfn twice[T: Area](x: T) -> Int { x.area() + Area.area(x) }\nfn main() -> Int { let q = Sq(s: 2); Sq(s: 6).area() + twice(q) + q.area() }",
        "trait Area { fn area(self: Self) -> Int; fn side(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Int { 1 } }\nfn main() -> Int { 0 }",
        "trait Area { fn area(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Int { 1 } fn zz(self: Sq) -> Int { 2 } }\nfn main() -> Int { 0 }",
        "trait Area { fn area(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Bool { true } }\nfn main() -> Int { 0 }",
        "trait Area { fn area(x: Int) -> Int; }\nfn main() -> Int { 0 }",
        "trait Area { fn area(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Int { 1 } }\nimpl Area for Sq { fn area(self: Sq) -> Int { 1 } }\nfn main() -> Int { 0 }",
        "struct Sq { s: Int }\nfn main() -> Int { Sq(s: 1).area() }",
        "trait A { fn m(self: Self) -> Int; }\ntrait B { fn m(self: Self) -> Int; }\nimpl A for Int { fn m(self: Int) -> Int { 1 } }\nimpl B for Int { fn m(self: Int) -> Int { 2 } }\nfn main() -> Int { 3.m() }",
        "trait Area { fn area(self: Self) -> Int; }\nfn f[T: Area](x: T) -> Int { x.area() }\nfn main() -> Int { f(3) }",
        "trait Area { fn area(self: Self) -> Int; }\nfn f(x: Area) -> Int { 0 }\nfn main() -> Int { 0 }",
        "trait Area { fn area(self: Self) -> Int; }\nimpl Area for Ints { fn area(self: Ints) -> Int { 0 } }\nfn main() -> Int { 0 }",
        "struct Sq { s: Int }\nimpl Nope for Sq { fn area(self: Sq) -> Int { 0 } }\nfn main() -> Int { 0 }",
        "fn f[T](x: T) -> Int { x.area() }\nfn main() -> Int { 0 }",
        "trait Say { fn say(self: Self) -> Int ! {}; }\nimpl Say for Int { fn say(self: Int) -> Int ! { io } { 0 } }\nfn main() -> Int { 0 }",
        // Closures and function values: accepted, and their refusals.
        "fn double(n: Int) -> Int { n * 2 }\nfn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }\nfn main() -> Int { let step = 3; let add = fn(x: Int) -> Int { x + step }; twice(double, 5) + twice(add, 1) }",
        // A `let mut` an `Int` closure captures is shared through a cell since R1b (L10), outside
        // the Nazm-written compiler's subset; one that can hold a function value is still refused.
        "fn main() -> Int { let mut g = fn(x: Int) -> Int { x }; let f = fn(x: Int) -> Int { g(x) }; f(1) }",
        "fn main() -> Int { let f = str_len; 0 }",
        "fn id[T](x: T) -> T { x }\nfn main() -> Int { let f = id; 0 }",
        "fn main() -> Int { let x = 3; x(1) }",
        "fn main() -> Int { let f = fn(x: Int) -> Int { x }; f(1, 2) }",
        "fn main(io: IoCap) -> Int ! { io } { let f = fn(x: Int) -> Int { print(\"a\"); x }; f(1) }",
        "fn main() -> Int { let f = fn(x: Int) -> Int { true }; f(1) }",
        // Authority is held, never inherited (N104): a built-in or a `spawn` with no capability
        // of its kind in scope, in a declared or an undeclared function, and what holds it.
        // (`main` itself holds an `OutCap` since R1, 5b.1: a function it calls does not.)
        "fn say() -> Int { print(\"x\"); 0 }\nfn main() -> Int { say() }",
        "fn shout() -> Int { print(\"!\"); 0 }\nfn main(io: IoCap) -> Int { shout() }",
        "fn shout(o: OutCap) -> Int { print(\"!\"); write_file(\"f\", \"x\"); print(\"?\"); 0 }\nfn main(io: IoCap) -> Int { shout(io) }",
        "fn shout(o: OutCap) -> Int { print(\"!\"); 0 }\nfn main(io: IoCap) -> Int { shout(io) }",
        "fn w(n: Int) -> Int { n }\nfn main() -> Int { scope { spawn w(1); } 0 }",
        "fn main(io: IoCap) -> Int { let f = fn(x: Int) -> Int ! { io } { print(\"a\"); x }; f(1) }",
        // `&&`, `||` and `!`.
        "fn main() -> Int { if 1 < 2 && 2 < 3 || false && !true { 7 } else { 8 } }",
        "fn main() -> Int { if 1 && true { 1 } else { 0 } }",
        "fn main() -> Int { if true || 2 { 1 } else { 0 } }",
        "fn main() -> Int { if !3 { 1 } else { 0 } }",
    ];
    let mut refusals = 0;
    for source in sources {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, source).expect("write");
        let (status, stdout, stderr) = run_stage(&exe, &file);
        let ours: Vec<(String, u32, u32)> = sorted_diagnostics(&stdout);
        let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(source)
            .into_iter()
            .map(|d| {
                (
                    d.code.as_str().to_owned(),
                    d.primary.span.start,
                    d.primary.span.end,
                )
            })
            .collect();
        theirs.sort();
        assert_eq!(ours, theirs, "checking differs for {source:?}\n{stderr}");
        if theirs.is_empty() {
            assert_eq!(
                status,
                Some(0),
                "{source:?} should have been accepted: {stdout}"
            );
        } else {
            refusals += 1;
            assert_eq!(status, Some(1), "{source:?} should have been refused");
        }
    }
    assert!(refusals >= 28, "only {refusals} refusals were compared");
}

/// N102: the same features compiled by the compiler written in Nazm and by the reference —
/// the same output, status, failure code and memory report, every kind reclaimed, closures
/// included — on stated answers, so two compilers that share a bug do not pass by agreeing.
#[test]
fn the_nazm_written_compiler_compiles_the_n102_features_with_the_same_memory_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    // A module beside every case, for the cross-module one: a trait, an impl and a function
    // taking a function value, used from the other file.
    std::fs::write(
        dir.path().join("shapes.nz"),
        "pub trait Area { fn area(self: Self) -> Int; }\npub struct Sq { s: Int }\n\
         impl Area for Sq { fn area(self: Sq) -> Int { self.s * self.s } }\n\
         pub fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n",
    )
    .expect("write");
    same_memory_semantics(
        dir.path(),
        &emitter,
        &[
            // Recursion, direct and mutual.
            ("fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\nfn main() -> Int { fib(15) }", "610"),
            ("fn ev(n: Int) -> Bool { if n == 0 { true } else { od(n - 1) } }\nfn od(n: Int) -> Bool { if n == 0 { false } else { ev(n - 1) } }\nfn main() -> Int { if ev(10) { 1 } else { 0 } }", "1"),
            // Effects and capabilities.
            ("fn main(io: IoCap) -> Int ! { io } { print(\"hi\"); 0 }", "hi0"),
            // Contracts, on every way out.
            ("fn isqrt(n: Int) -> Int ! {}\n    requires n >= 0\n    ensures result * result <= n\n{ let mut r = 0; while (r + 1) * (r + 1) <= n { r = r + 1; } r }\nfn main() -> Int { isqrt(50) }", "7"),
            ("fn s(n: Int) -> Str ! {}\n    ensures str_len(result) > 0\n{ if n > 99 { return int_to_str(n); } int_to_str(n + 1) }\nfn main(io: IoCap) -> Int ! { io } { print(s(12)); print(s(120)); 0 }", "131200"),
            // A foreign declaration.
            ("extern \"C\" fn labs(x: Int) -> Int = \"labs\";\nfn main(f: ForeignCap) -> Int ! { foreign } { labs(0 - 12) }", "12"),
            // A standard module.
            ("use \"@std/num\";\nfn main() -> Int { num_abs(0 - 9) + num_min(4, 2) }", "11"),
            // Traits: a concrete call, a call through a bound, a call naming the trait, and
            // across modules.
            ("trait Area { fn area(self: Self) -> Int; }\nstruct Sq { s: Int }\nimpl Area for Sq { fn area(self: Sq) -> Int { self.s * self.s } }\nfn twice[T: Area](x: T) -> Int { x.area() + Area.area(x) }\nfn main() -> Int { let q = Sq(s: 2); Sq(s: 6).area() + twice(q) + q.area() }", "48"),
            ("trait Show { fn show(self: Self) -> Str; }\nimpl Show for Int { fn show(self: Int) -> Str { int_to_str(self) } }\nfn main(io: IoCap) -> Int ! { io } { print(5.show()); 0 }", "50"),
            ("use \"shapes.nz\";\nfn main() -> Int { Sq(s: 3).area() + apply(fn(x: Int) -> Int { x + 1 }, 4) }", "14"),
            // Closures: captures, a string captured and released, nested capture, a closure
            // returned and copied, a named function as a value.
            ("fn double(n: Int) -> Int { n * 2 }\nfn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }\nfn main() -> Int { let step = 3; let add = fn(x: Int) -> Int { x + step }; twice(double, 5) + twice(add, 1) }", "27"),
            ("fn main(io: IoCap) -> Int ! { io } { let s = int_to_str(42); let f = fn(x: Int) -> Int { str_len(s) + x }; print(s); f(1) }", "423"),
            ("fn main() -> Int { let a = 2; let f = fn(x: Int) -> Int { let g = fn(y: Int) -> Int { y + a }; g(x) }; f(1) }", "3"),
            ("fn mk(n: Int) -> fn(Int) -> Int { fn(x: Int) -> Int { x * n } }\nfn main() -> Int { let t = mk(3); let u = t; u(7) + t(1) }", "24"),
            // A captured string outlives the closure that captured it: the closure holds its
            // own reference, so releasing the closure leaves `s` alive, and a string made
            // after cannot take its storage.
            ("fn once(f: fn(Int) -> Int) -> Int { f(1) }\nfn main(io: IoCap) -> Int ! { io } { let s = int_to_str(42); let n = once(fn(x: Int) -> Int { str_len(s) + x }); let t = int_to_str(77); print(s); print(t); n }", "42773"),
            // `&&`, `||` and `!`, evaluated only as far as they must be.
            ("fn boom() -> Bool { 1 / 0 == 1 }\nfn main() -> Int { if false && boom() { 1 } else { if true || boom() { 2 } else { 3 } } }", "2"),
            ("fn f(x: Int) -> Bool { x > 3 && x < 9 || x == 0 }\nfn main() -> Int { let mut n = 0; let mut i = 0; while i < 12 { if f(i) { n = n + 1; } if !f(i) { n = n + 100; } i = i + 1; } n }", "606"),
        ],
    );
    // And the failures: each at its code, from both compilers' programs.
    for (source, code) in [
        ("fn down(n: Int) -> Int { down(n + 1) + 1 }\nfn main() -> Int { down(0) }", "N0408"),
        ("fn f(n: Int) -> Int ! {}\n    requires n >= 0\n{ n }\nfn neg() -> Int { 0 - 1 }\nfn main() -> Int { f(neg()) }", "N0410"),
        ("fn f(n: Int) -> Int ! {}\n    ensures result > n\n{ if n > 5 { return n + 1; } n }\nfn main() -> Int { f(9) + f(3) }", "N0411"),
        ("fn boom(x: Int) -> Int { x * 9223372036854775807 }\nfn ap(f: fn(Int) -> Int) -> Int { f(2) }\nfn main() -> Int { ap(boom) }", "N0400"),
    ] {
        let file = dir.path().join("fail.nz");
        std::fs::write(&file, source).expect("write");
        let (rc, ir, err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(0), "{source}: {err}");
        let ll = dir.path().join("fail.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let exe = dir.path().join("fail-self");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-lpthread", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let reference = dir.path().join("fail-ref");
        let out = Command::new(NAZM)
            .args(["build", file.to_str().unwrap(), "-o"])
            .arg(&reference)
            .output()
            .expect("run nazm build");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let theirs = run_native(&exe);
        let ours = run_native(&reference);
        assert_eq!(theirs, ours, "{source}");
        assert_eq!(theirs.0, Some(2), "{source}");
        assert_eq!(theirs.2.as_deref(), Some(code), "{source}");
    }
}

/// N103: re-exports through the compiler written in Nazm — the same programs, the same answers
/// and memory reports as the reference, and the same refusals at the same positions.
#[test]
fn the_nazm_written_compiler_agrees_with_the_reference_on_reexports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    for (name, text) in [
        (
            "shapes.nz",
            "pub trait Area { fn area(self: Self) -> Int; }\npub struct Sq { s: Int }\n\
             impl Area for Sq { fn area(self: Sq) -> Int { self.s * self.s } }\n\
             pub fn double(n: Int) -> Int { n * 2 }\nfn hidden() -> Int { 9 }\n",
        ),
        (
            "facade.nz",
            "pub use \"shapes.nz\" { Sq as Square, double as twice, Area };\n",
        ),
        ("all.nz", "pub use \"shapes.nz\";\n"),
        ("cycb.nz", "pub use \"mem.nz\";\npub fn b() -> Int { 2 }\n"),
    ] {
        std::fs::write(dir.path().join(name), text).expect("write");
    }
    same_memory_semantics(
        dir.path(),
        &emitter,
        &[
            ("use \"facade.nz\";\nfn main() -> Int { Square(s: 3).area() + twice(4) }", "17"),
            ("use \"all.nz\";\nfn tw[T: Area](x: T) -> Int { x.area() }\nfn main() -> Int { Sq(s: 2).area() + double(1) + tw(Sq(s: 1)) }", "7"),
        ],
    );
    // The refusals, from both front ends, by code and position.
    for (source, expected) in [
        (
            "pub use \"shapes.nz\" { hidden, nope, double };\npub fn double(n: Int) -> Int { n }\nfn main() -> Int { 0 }\n",
            &["N0211", "N0211", "N0212", "N0208"][..],
        ),
        ("pub use \"shapes.nz\" { Area as Shape };\nfn main() -> Int { 0 }\n", &["N0211"][..]),
        ("pub use \"cycb.nz\";\nfn main() -> Int { 0 }\n", &["N0213"][..]),
    ] {
        let file = dir.path().join("mem.nz");
        std::fs::write(&file, source).expect("write");
        let (rc, _, err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(1), "{source}: {err}");
        let ours: Vec<&str> = err
            .lines()
            .filter_map(|l| l.split("error[").nth(1).and_then(|r| r.split(']').next()))
            .collect();
        let out = Command::new(NAZM)
            .args(["check", file.to_str().unwrap()])
            .output()
            .expect("run nazm check");
        let reference = String::from_utf8_lossy(&out.stderr);
        let theirs: Vec<&str> = reference
            .lines()
            .filter_map(|l| l.strip_prefix("error[").and_then(|r| r.split(']').next()))
            .collect();
        assert!(ours.starts_with(expected) || ours == expected, "{source}: {ours:?}");
        // The root's own diagnostics agree; the reference also reports the other module's
        // half of a cycle, which the Nazm-written compiler reports only for the root.
        for code in expected {
            assert!(theirs.contains(code), "{source}: the reference said {theirs:?}");
        }
    }
}

#[test]
fn the_nazm_written_compiler_compiles_requirements_with_the_same_memory_semantics() {
    // N14: bounded generic bodies instantiated at owning types, compiled by the
    // Nazm-written compiler against the reference, on the stated answers and on
    // reclamation.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let prelude = "fn churn() -> Int { let mut noise = vec_new[Str](); let mut k = 0; \
                   while k < 64 { vec_push(noise, str_concat(\"noise-\", int_to_str(k))); k = k + 1; } \
                   noise = vec_new[Str](); vec_len(noise) }\n\
                   fn same[T: Equality](a: T, b: T) -> Bool { a == b }\n\
                   fn wrap[T: Equality](a: T, b: T) -> Bool { same(a, b) }\n\
                   fn bit(b: Bool, n: Int) -> Int { if b { n } else { 0 } }\n\
                   fn text(n: Int) -> Str { str_concat(\"t-\", int_to_str(n)) }\n\
                   struct Box[T] { value: T, }\n\
                   struct Item { name: Str, n: Int, }\n\
                   enum Shape { Plain, Named(label: Str), }\n\
                   fn box_same[T: Equality](a: Box[T], b: Box[T]) -> Bool { a == b }\n";
    let cases: Vec<(String, &str)> = vec![
        (format!("{prelude}fn main() -> Int {{ bit(same(1, 1), 1) + bit(same(text(1), \"t-2\"), 2) \
             + bit(same(Item(name: text(3), n: 3), Item(n: 3, name: \"t-3\")), 4) + bit(same(Shape.Plain(), Shape.Named(label: text(4))), 8) \
             + bit(same(Option[Int].Some(value: 5), Option[Int].Some(value: 5)), 16) \
             + bit(wrap(Result[Str, Int].Ok(value: text(6)), Result[Str, Int].Ok(value: text(6))), 32) \
             + bit(box_same(Box[Shape](value: Shape.Named(label: text(7))), Box[Shape](value: Shape.Named(label: text(7)))), 64) }}\n"),
         "117"),
        (format!("{prelude}fn main() -> Int {{ let mut total = 0; let mut i = 0; while i < 5 {{ \
             total = total + bit(same(text(i), text(i)), 1) + bit(wrap(Box[Str](value: text(i)), Box[Str](value: text(i + 1))), 10); \
             churn(); i = i + 1; }} let a = Item(name: text(8), n: 8); let s = same(a, a); churn(); total + bit(s, 100) + 1000 * str_len(a.name) }}\n"),
         "3105"),
    ];
    let borrowed: Vec<(&str, &str)> = cases.iter().map(|(s, e)| (s.as_str(), *e)).collect();
    same_memory_semantics(dir.path(), &emitter, &borrowed);
}

/// Build a harness that checks a three-module program — `root`, `dep` and the core
/// prelude — through `check_program` directly, with the prelude either where the loader
/// puts it or between the two project modules.
fn build_layout_harness(dir: &Path) -> PathBuf {
    common::require_containment("analyse.nz");
    for f in ["diag.nz", "lex.nz", "parse.nz", "module.nz", "analyse.nz"] {
        std::fs::copy(repo_root().join("compiler").join(f), dir.join(f)).expect("copy");
    }
    std::fs::write(
        dir.join("layout.nz"),
        "use \"analyse.nz\";\nuse \"module.nz\";\n\
         // arg(0) the prelude, arg(1) the root, arg(2) its dependency, arg(3) `last`, `between` or `none`.\n\
         fn main(io: IoCap) -> Int {\n    let core = arg(0);\n    let root = read_file(arg(1));\n    let dep = read_file(arg(2));\n    \
         let nl = str_from_byte(10);\n    let mpath = strs_new();\n    let mstart = ints_new();\n    let mfirst = ints_new();\n    \
         let mcount = ints_new();\n    let mto = ints_new();\n    let mat = ints_new();\n    let mut src = \"\";\n    \
         if arg(3) != \"between\" {\n        \
         // The loader's layout: root, dep, then `attach_core` — or, for `none`, no prelude.\n        \
         strs_push(mpath, \"root.nz\");\n        strs_push(mpath, \"dep.nz\");\n        \
         ints_push(mstart, 0);\n        ints_push(mstart, str_len(root) + 1);\n        \
         ints_push(mfirst, 0);\n        ints_push(mcount, 1);\n        ints_push(mto, 1);\n        ints_push(mat, 0);\n        \
         ints_push(mfirst, 1);\n        ints_push(mcount, 0);\n        \
         src = str_concat(str_concat(root, nl), dep);\n        \
         if arg(3) == \"last\" {\n            src = attach_core(core, src, mpath, mstart, mfirst, mcount, mto, mat, io);\n        }\n    \
         } else {\n        \
         // The prelude between the two, keyed as the prelude, so it is neither last nor\n        \
         // any particular index the checker could assume.\n        \
         let text = read_file(core);\n        \
         strs_push(mpath, \"root.nz\");\n        strs_push(mpath, core_key());\n        strs_push(mpath, \"dep.nz\");\n        \
         ints_push(mstart, 0);\n        ints_push(mstart, str_len(root) + 1);\n        \
         ints_push(mstart, str_len(root) + 1 + str_len(text) + 1);\n        \
         ints_push(mfirst, 0);\n        ints_push(mcount, 2);\n        ints_push(mto, 2);\n        ints_push(mat, 0);\n        \
         ints_push(mto, 1);\n        ints_push(mat, 0 - 1);\n        \
         ints_push(mfirst, 2);\n        ints_push(mcount, 0);\n        \
         ints_push(mfirst, 2);\n        ints_push(mcount, 1);\n        ints_push(mto, 1);\n        ints_push(mat, 0 - 1);\n        \
         src = str_concat(str_concat(str_concat(str_concat(root, nl), text), nl), dep);\n    }\n    \
         let out = strs_new();\n    \
         match check_program(src, mpath, mstart, mfirst, mcount, mto, mat, io) {\n        \
         Result.Ok(value: checked) => {\n            strs_push(out, \"ok\");\n        },\n        \
         Result.Err(error: found) => {\n            let mut d = 0;\n            while d < vec_len(found) {\n                \
         strs_push(out, vec_get(found, d).code);\n                d = d + 1;\n            }\n        },\n    };\n    \
         print(str_concat(str_join(out, \",\"), nl));\n    0\n}\n",
    )
    .expect("write harness");
    let exe = dir.join("layout");
    let out = Command::new(NAZM)
        .args(["build", dir.join("layout.nz").to_str().unwrap(), "-o"])
        .arg(&exe)
        .output()
        .expect("run nazm build");
    assert!(
        out.status.success(),
        "the layout harness does not compile: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

#[test]
fn the_nazm_checker_finds_the_prelude_by_its_key_wherever_it_is_loaded() {
    // N12.1 §4–§11. The Nazm-written checker used to take the *last* module as the core
    // prelude. Here the same three modules are checked in the loader's order and with the
    // prelude between the two project modules, and one case with no prelude at all. Every
    // answer that depends on which module is the prelude must be the same: which `Result`
    // a `?` accepts, which names are reserved, and that a lookalike is not one.
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = build_layout_harness(dir.path());
    let core = repo_root().join("library/core/prelude.nz");
    let root_q = "use \"dep.nz\";\nfn f(r: Result[Int, Str]) -> Result[Int, Str] { let n = r?; Result[Int, Str].Ok(value: n + helper()) }\n\
                  fn main() -> Int { match f(Result[Int, Str].Ok(value: 1)) { Result.Ok(value: v) => { let w = v; w }, Result.Err(error: e) => 0, } }\n";
    let cases: [(&str, &str, &str); 5] = [
        // `?` over the prelude's `Result`, and `Option` through the same module.
        (
            root_q,
            "pub fn helper() -> Int { match Option[Int].Some(value: 1) { Option.None() => 0, Option.Some(value: v) => v, } }\n",
            "ok",
        ),
        // A dependency declaring the prelude's name is refused at its declaration, and the
        // prelude's meaning stands for the root.
        (
            root_q,
            "pub enum Result[T, E] { Ok(value: T), Err(error: E), }\npub fn helper() -> Int { 1 }\n",
            "N0363",
        ),
        (
            root_q,
            "pub struct Option { x: Int, }\npub fn helper() -> Int { 1 }\n",
            "N0363",
        ),
        // A lookalike with the right shape and the wrong definition.
        (
            "use \"dep.nz\";\nfn f() -> Result[Int, Str] { let n = mk()?; Result[Int, Str].Ok(value: n) }\nfn main() -> Int { 0 }\n",
            "pub enum MyResult[T, E] { Ok(value: T), Err(error: E), }\npub fn mk() -> MyResult[Int, Str] { MyResult[Int, Str].Ok(value: 1) }\n",
            "N0360",
        ),
        // `?` on an `Int`. Refused wherever the prelude is — and also with no prelude at
        // all, where there is no `Result` to be: "none" must not compare equal to anything.
        (
            "use \"dep.nz\";\nfn main() -> Int { let n = 4; let m = n?; m }\n",
            "pub fn helper() -> Int { 1 }\n",
            "N0360",
        ),
    ];
    for (root, dep, expected) in cases {
        std::fs::write(dir.path().join("root.nz"), root).expect("write");
        std::fs::write(dir.path().join("dep.nz"), dep).expect("write");
        // Only the last case names no prelude type, so only it can be checked without one.
        let layouts: &[&str] = if root.contains("n?") {
            &["last", "between", "none"]
        } else {
            &["last", "between"]
        };
        for layout in layouts {
            let (status, stdout, stderr) = run_bounded(
                common::bounded_exe(&exe)
                    .arg(&core)
                    .arg(dir.path().join("root.nz"))
                    .arg(dir.path().join("dep.nz"))
                    .arg(layout),
                STAGE_LIMIT,
            )
            .expect("the harness hung");
            assert_eq!(status, Some(0), "{layout}: {stderr}");
            assert_eq!(
                stdout.lines().next().unwrap_or(""),
                expected,
                "with the prelude {layout}, for:\n{root}\n{dep}"
            );
        }
    }
}

#[test]
fn a_project_file_cannot_take_the_prelude_s_key() {
    // N12.1 §9. A project file whose path *is* `@core/prelude` would, keyed by its path,
    // hold the prelude's key. It is a project file like any other — it compiles, as the
    // reference compiles one under `@core/` — but under a key outside that namespace, so
    // the `Result` its `?` accepts is still the prelude's. In the checker's driver and in
    // the compiler's, and the compiled program agrees with the reference's.
    let dir = tempfile::tempdir().expect("tempdir");
    let checker = build_checker(dir.path());
    let emitter = build_emitter(dir.path());
    std::fs::create_dir_all(dir.path().join("@core")).expect("mkdir");
    std::fs::write(
        dir.path().join("@core/prelude"),
        "fn f(r: Result[Int, Str]) -> Result[Int, Str] { let n = r?; Result[Int, Str].Ok(value: n + 1) }\n\
         fn main() -> Int { match f(Result[Int, Str].Ok(value: 41)) { Result.Ok(value: v) => v, Result.Err(error: e) => 0, } }\n",
    )
    .expect("write");
    let core = repo_root().join("library/core/prelude.nz");
    let stage = |exe: &Path| {
        run_bounded(
            common::bounded_exe(exe)
                .current_dir(dir.path())
                .arg("@core/prelude")
                .arg(&core),
            STAGE_LIMIT,
        )
        .expect("the stage hung")
    };

    let (status, stdout, stderr) = stage(&checker);
    assert_eq!(status, Some(0), "the checker: {stdout}{stderr}");
    assert!(!stdout.contains('|'), "the checker reported: {stdout}");

    let (status, ir, stderr) = stage(&emitter);
    assert_eq!(status, Some(0), "the compiler: {stderr}");
    let ll = dir.path().join("imp.ll");
    std::fs::write(&ll, &ir).expect("write IR");
    let exe = dir.path().join("imp");
    let built = Command::new("clang")
        .args(["-Wno-override-module", "-lpthread", "-o"])
        .arg(&exe)
        .arg(&ll)
        .output()
        .expect("run clang");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let (_, out, _) = run_bounded(&mut common::bounded_exe(&exe), STAGE_LIMIT).expect("hung");
    let reference = Command::new(NAZM)
        .args(["run", "--interpret", "@core/prelude"])
        .current_dir(dir.path())
        .output()
        .expect("run nazm");
    assert_eq!(out.trim(), "42");
    assert_eq!(String::from_utf8_lossy(&reference.stdout).trim(), "42");
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
        let (_, stdout, _) = run_stage(&exe, file);
        let ours = sorted_diagnostics(&stdout);
        // A file that `use`s another is resolved by the driver, not by this checker, so
        // its imported names look undefined in isolation. Those files are checked as a
        // whole program by `nazm check`, which is what the assertion below uses.
        let source = std::fs::read_to_string(file).expect("read");
        // Concurrency sources are no longer skipped: the Nazm checker types `Chan`, knows
        // the four `chan_*` built-ins, and applies the two rules a task has —
        // `spawn` needs a `scope` (N0320) and a sequence may not cross into one (N0321).
        let standalone = !source.contains("use \"");
        if standalone {
            // Compared against the reference rather than asserted to be empty. The old
            // assertion said "a source the reference accepts" and never asked it, which
            // was wrong the moment either checker gained a rule about whole files: a
            // library module has no `main`, and `nazm check` has always said so.
            let mut theirs: Vec<(String, u32, u32)> = nazm_core::check_source(&source)
                .into_iter()
                .filter(|d| COMPARABLE.contains(&d.code.as_str()))
                .map(|d| {
                    (
                        d.code.as_str().to_owned(),
                        d.primary.span.start,
                        d.primary.span.end,
                    )
                })
                .collect();
            theirs.sort();
            assert_eq!(
                ours,
                theirs,
                "{}: the two checkers disagree",
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
    // The built-ins the compiler written in Nazm deliberately does not carry, each with why.
    // Not an exclusion list in the Nazm source — that stays forbidden below — but this test's
    // own record, so an omission is a decision someone wrote down and not a gap.
    const NOT_CARRIED: &[(&str, &str)] = &[
        // N53: `Chan[T]` is a type form the Nazm checker's type table does not have, and its
        // runtime is outside the parity gate. A program using one is refused by name there
        // (an unknown function) rather than mistyped.
        ("chan_new_of", "Chan[T]"),
        ("chan_send_of", "Chan[T]"),
        ("chan_recv_of", "Chan[T]"),
        ("chan_close_of", "Chan[T]"),
        // N54: a select takes a `Vec[Chan[T]]`; the clock is not carried either — the Nazm
        // checker has the `TimeCap` type since N102, not the built-in. Refused by name there too.
        ("chan_select_of", "Vec[Chan[T]]"),
        ("chan_select_until", "Vec[Chan[T]]"),
        ("time_now_ms", "TimeCap"),
        // N62: a device register needs an `MmioCap`, and exists only on a freestanding target
        // the compiler written in Nazm does not build for.
        ("mmio_read32", "MmioCap"),
        ("mmio_write32", "MmioCap"),
        // N66: the ordered sum is a native build's vectorisation target; the compiler written
        // in Nazm sums with its own loops and has no use for it.
        ("ints_sum_from", "vectorisation"),
        // N85: `c_errno` reads what a foreign call left. The compiler written in Nazm calls C
        // since N102 (`Int` only) but does not read `errno`; a call is an unknown function there.
        ("c_errno", "ForeignCap"),
        // N86: the device registers by width, as their 32-bit pair (N62).
        ("mmio_read8", "MmioCap"),
        ("mmio_write8", "MmioCap"),
        ("mmio_read16", "MmioCap"),
        ("mmio_write16", "MmioCap"),
        // Gate 3: the 64-bit pair, as the narrower ones.
        ("mmio_read64", "MmioCap"),
        ("mmio_write64", "MmioCap"),
        // Gate 3: a board's watchdog, a device's authority as the registers are.
        ("watchdog_feed", "MmioCap"),
        // R1a: immutable indirection, outside the Nazm-written compiler's type table until R2.
        ("box_new", "Box[T]"),
        ("box_get", "Box[T]"),
        ("list_from_vec", "List[T]"),
        ("list_empty", "List[T]"),
        ("list_len", "List[T]"),
        ("list_get", "List[T]"),
        // R1b: a mutable cell, likewise.
        ("cell_new", "Cell[T]"),
        ("cell_get", "Cell[T]"),
        ("cell_set", "Cell[T]"),
        // R1b: checked arithmetic, generic over the integers, likewise.
        ("checked_add", "Option[T]"),
        ("checked_sub", "Option[T]"),
        ("checked_mul", "Option[T]"),
        // R1b (L13b): `UInt64` and `UInt128` are Gate 2 numbers, which the Nazm checker's type
        // table does not have, so this is refused by name there too.
        ("widening_mul", "Gate 2 numbers"),
        // Gate 2 (numbers): the Nazm checker's type table has `Int` alone, so these, generic over
        // the widths and floats, are unknown functions there and refused by name.
        ("wrapping_add", "Gate 2 numbers"),
        ("wrapping_sub", "Gate 2 numbers"),
        ("wrapping_mul", "Gate 2 numbers"),
        ("saturating_add", "Gate 2 numbers"),
        ("saturating_sub", "Gate 2 numbers"),
        ("saturating_mul", "Gate 2 numbers"),
        ("count_ones", "Gate 2 numbers"),
        ("leading_zeros", "Gate 2 numbers"),
        ("trailing_zeros", "Gate 2 numbers"),
        ("rotate_left", "Gate 2 numbers"),
        ("rotate_right", "Gate 2 numbers"),
        ("swap_bytes", "Gate 2 numbers"),
        ("to_big_endian", "Gate 2 numbers"),
        ("from_big_endian", "Gate 2 numbers"),
        ("to_little_endian", "Gate 2 numbers"),
        ("from_little_endian", "Gate 2 numbers"),
        ("float_sqrt", "Gate 2 numbers"),
        ("float_abs", "Gate 2 numbers"),
        ("float_floor", "Gate 2 numbers"),
        ("float_ceil", "Gate 2 numbers"),
        ("float_trunc", "Gate 2 numbers"),
        ("float_round", "Gate 2 numbers"),
        ("float_sin", "Gate 2 numbers"),
        ("float_cos", "Gate 2 numbers"),
        ("float_tan", "Gate 2 numbers"),
        ("float_exp", "Gate 2 numbers"),
        ("float_ln", "Gate 2 numbers"),
        ("float_log2", "Gate 2 numbers"),
        ("float_log10", "Gate 2 numbers"),
        ("float_pow", "Gate 2 numbers"),
        ("float_atan2", "Gate 2 numbers"),
        ("float_min", "Gate 2 numbers"),
        ("float_max", "Gate 2 numbers"),
        ("float_is_nan", "Gate 2 numbers"),
        ("float_is_infinite", "Gate 2 numbers"),
        ("float_is_finite", "Gate 2 numbers"),
        ("num_to_str", "Gate 2 numbers"),
        ("float_to_str_fixed", "R1a interpolation (L4)"),
        ("str_pad_start", "R1a interpolation (L4)"),
        ("str_pad_end", "R1a interpolation (L4)"),
        ("float64_to_bits", "Gate 2 numbers"),
        ("float64_from_bits", "Gate 2 numbers"),
        ("float32_to_bits", "Gate 2 numbers"),
        ("float32_from_bits", "Gate 2 numbers"),
        ("float_parses", "Gate 2 numbers"),
        ("float_parse_raw", "Gate 2 numbers"),
        // Gate 2 (bytes and text): `Bytes` and `Text` are not in the Nazm checker's type table.
        ("bytes_new", "Gate 2 Bytes and Text"),
        ("bytes_from_str", "Gate 2 Bytes and Text"),
        ("bytes_len", "Gate 2 Bytes and Text"),
        ("bytes_get", "Gate 2 Bytes and Text"),
        ("bytes_set", "Gate 2 Bytes and Text"),
        ("bytes_slice", "Gate 2 Bytes and Text"),
        ("bytes_to_str", "Gate 2 Bytes and Text"),
        ("bytes_copy", "Gate 2 Bytes and Text"),
        ("bytes_read_le", "Gate 2 Bytes and Text"),
        ("bytes_read_be", "Gate 2 Bytes and Text"),
        ("bytes_write_le", "Gate 2 Bytes and Text"),
        ("bytes_write_be", "Gate 2 Bytes and Text"),
        ("utf8_valid", "Gate 2 Bytes and Text"),
        ("text_as_str", "Gate 2 Bytes and Text"),
        ("text_len", "Gate 2 Bytes and Text"),
        ("text_concat", "Gate 2 Bytes and Text"),
        ("text_is_boundary", "Gate 2 Bytes and Text"),
        ("text_slice", "Gate 2 Bytes and Text"),
        ("text_scalar_at", "Gate 2 Bytes and Text"),
        ("text_next", "Gate 2 Bytes and Text"),
        ("text_scalar_count", "Gate 2 Bytes and Text"),
        ("scalar_valid", "Gate 2 Bytes and Text"),
        ("scalar_text", "Gate 2 Bytes and Text"),
        // Gate 2 (files and handles): `OsHandle` is not in the Nazm checker's type table.
        ("os_open_read", "Gate 2 files and handles"),
        ("os_open_write", "Gate 2 files and handles"),
        ("os_dir_open", "Gate 2 files and handles"),
        ("os_stdin", "Gate 2 files and handles"),
        ("os_status", "Gate 2 files and handles"),
        ("os_read", "Gate 2 files and handles"),
        // R1d, P3: a line of standard input, a handle's as `os_read` is.
        ("os_read_line", "R1d streaming standard input"),
        ("os_write", "Gate 2 files and handles"),
        ("os_seek", "Gate 2 files and handles"),
        ("os_sync", "Gate 2 files and handles"),
        ("os_close", "Gate 2 files and handles"),
        ("os_dir_next", "Gate 2 files and handles"),
        ("os_stat", "Gate 2 files and handles"),
        ("os_mkdir", "Gate 2 files and handles"),
        ("os_rmdir", "Gate 2 files and handles"),
        ("os_unlink", "Gate 2 files and handles"),
        ("os_rename", "Gate 2 files and handles"),
        ("os_family", "Gate 2 files and handles"),
        ("os_error_text", "Gate 2 files and handles"),
        // Gate 2 (networking): neither `OsHandle` nor `NetCap` is in the Nazm checker's table.
        ("os_tcp_listen", "Gate 2 networking"),
        ("os_tcp_accept", "Gate 2 networking"),
        ("os_tcp_connect", "Gate 2 networking"),
        ("os_udp_bind", "Gate 2 networking"),
        ("os_udp_send_to", "Gate 2 networking"),
        ("os_udp_recv_from", "Gate 2 networking"),
        ("os_set_timeout", "Gate 2 networking"),
        ("os_shutdown", "Gate 2 networking"),
        ("os_local_addr", "Gate 2 networking"),
        ("os_peer_addr", "Gate 2 networking"),
        ("os_resolve", "Gate 2 networking"),
        // Gate 2 (time): the self-hosted checker carries `time_now_ms` and no other clock.
        ("time_now_ns", "Gate 2 time"),
        ("time_wall_ns", "Gate 2 time"),
        ("time_sleep_ms", "Gate 2 time"),
        // Gate 2 (process and environment): neither `OsHandle` nor `ProcessCap` is in the Nazm
        // checker's table.
        ("os_env_get", "Gate 2 process and environment"),
        ("os_env_has", "Gate 2 process and environment"),
        ("os_env_count", "Gate 2 process and environment"),
        ("os_env_at", "Gate 2 process and environment"),
        ("os_spawn", "Gate 2 process and environment"),
        ("os_child_pipe", "Gate 2 process and environment"),
        ("os_wait", "Gate 2 process and environment"),
        ("os_kill", "Gate 2 process and environment"),
        // R1d (the platform, further): neither `OsHandle` nor `ProcessCap` is in the Nazm
        // checker's table, as for Gate 2's.
        ("os_stdout", "R1d platform"),
        ("os_stderr", "R1d platform"),
        ("os_spawn_in", "R1d platform"),
        ("os_spawn_with", "R1d platform"),
        ("os_pid", "R1d platform"),
        ("os_child_pid", "R1d platform"),
        ("os_kill_group", "R1d platform"),
        ("os_try_wait", "R1d platform"),
        ("os_wait_until", "R1d platform"),
        ("os_child_rusage", "R1d platform"),
        ("os_lstat", "R1d platform"),
        ("os_chmod", "R1d platform"),
        ("os_realpath", "R1d platform"),
        ("os_mkdtemp", "R1d platform"),
        ("os_lock", "R1d platform"),
        ("os_try_lock", "R1d platform"),
        ("os_unlock", "R1d platform"),
        // Gate 2 (randomness): `RandomCap` is not in the Nazm checker's table.
        ("os_random_bytes", "Gate 2 randomness"),
        // Gate 3 (atomics): `Atomic` is not in the Nazm checker's table.
        ("atomic_new", "Gate 3 atomics"),
        ("atomic_load", "Gate 3 atomics"),
        ("atomic_store", "Gate 3 atomics"),
        ("atomic_add", "Gate 3 atomics"),
        ("atomic_swap", "Gate 3 atomics"),
        ("atomic_compare_swap", "Gate 3 atomics"),
    ];
    let carried = |i: &nazm_core::Intrinsic| !NOT_CARRIED.iter().any(|(n, _)| *n == i.name());
    for (name, _) in NOT_CARRIED {
        assert!(
            nazm_core::Intrinsic::ALL.iter().any(|i| i.name() == *name),
            "`{name}` is recorded as not carried, but is not a built-in"
        );
        assert!(
            !source.contains(&format!("\"{name}\"")),
            "`{name}` is now in compiler/analyse.nz: take it off the list"
        );
    }
    for i in nazm_core::Intrinsic::ALL.iter().filter(|i| carried(i)) {
        let needle = format!("\"{}\"", i.name());
        assert!(
            source.contains(&needle),
            "`{}` has no signature in compiler/analyse.nz — the front end's built-ins \
             and the Nazm checker's table have drifted",
            i.name()
        );
    }
    // There is no exclusion list any more. `chan_new`, `chan_send`, `chan_recv` and
    // `chan_close` were named in one so that a deliberate omission could not be mistaken
    // for a gap; the Nazm checker now types `Chan` and registers all four, so every
    // built-in the front end has must be registered here — no exceptions to keep track
    // of. If one is ever left out again it will fail below as "not registered".
    assert!(
        !source.contains("fn outside_bootstrap_subset"),
        "the exclusion list is back; if a built-in is deliberately not lowered, this \
         test needs to know which and why"
    );
    // The arity the Nazm table records must match, since a wrong one turns a valid call
    // into `N0303`. Checked by counting the non-`n` arguments on each line.
    for i in nazm_core::Intrinsic::ALL.iter().filter(|i| carried(i)) {
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
// compiler — which is what `docs/bootstrap.md` §4 records and a wrapper
// shelling out to Rust is not.
// ---------------------------------------------------------------------------

/// Run a command, killing it if it outlives `limit`.
///
/// Its own copy rather than one shared with `tests/build.rs`: each integration test is a
/// separate binary, and a compiled program that does not terminate would otherwise hang
/// the suite rather than fail it.
/// What the parent will buffer from one stream before it refuses.
///
/// Generous on purpose — the largest IR any test here produces is a few hundred
/// kilobytes — because this is not a budget. It is a refusal to reproduce the
/// 2026-09-21 accumulation bug one level up: `wait_with_output` buffers without limit,
/// and an unbounded stage writing into a pipe is exactly the shape that got us here.
const OUTPUT_CAP: usize = 64 * 1024 * 1024;

/// Runs `cmd` under a wall-clock deadline, draining both pipes while it runs. `None`
/// means the deadline passed first.
///
/// # Both pipes are drained on their own threads, and that is not a nicety
///
/// A child that writes more than the pipe buffer — 64 KiB on Linux — blocks until
/// something reads. A parent that waits for exit *first* and collects afterwards
/// therefore deadlocks, and then reports it as a hang, which is a wrong diagnosis of its
/// own bug. Every compiler stage here writes its output to stdout, so for this suite that
/// is the ordinary case rather than the pathological one.
///
/// # Cleanup
///
/// On the deadline the child is killed and reaped, and both readers are joined — the
/// kill closes the pipes, so they end. A test that leaves a stage running is a test that
/// competes with the next one, which is how ten of them were running at once on
/// 2026-09-21.
fn run_bounded(
    cmd: &mut Command,
    limit: std::time::Duration,
) -> Option<(Option<i32>, String, String)> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0_u8; 8192];
            while let Ok(n) = r.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let room = OUTPUT_CAP.saturating_sub(buf.len());
                assert!(
                    room > 0,
                    "run_bounded: a child wrote more than {OUTPUT_CAP} bytes to one \
                     stream. Refusing to buffer it: that is the accumulation bug this \
                     suite exists to catch, not something to grow a Vec for."
                );
                buf.extend_from_slice(chunk.get(..n.min(room)).unwrap_or_default());
            }
            buf
        })
    }

    // Its own process group, so the deadline reaches what it started as well: a script's
    // stage, or a stage's program, still holding the pipes would keep both readers — and
    // this function — waiting after the child itself was killed.
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .expect("spawn");
    let out = drain(child.stdout.take().expect("stdout pipe"));
    let err = drain(child.stderr.take().expect("stderr pipe"));

    let start = std::time::Instant::now();
    let finished = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break Some(status),
            None if start.elapsed() >= limit => {
                let _ = Command::new("kill")
                    .arg("-KILL")
                    // `--`: procps `kill` (the container's) parses `-<pgid>` after a signal as an
                    // option and signals nothing without it. BSD `kill` accepts both.
                    .arg("--")
                    .arg(format!("-{}", child.id()))
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    };
    // Joined either way, so nothing is left reading a pipe after the test moves on.
    let stdout = String::from_utf8_lossy(&out.join().expect("stdout reader")).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().expect("stderr reader")).into_owned();
    Some((finished?.code(), stdout, stderr))
}

/// The deadline reaches what the child started, not only the child.
///
/// Found in N12.2, not N12.1: the group kill above was written `kill -KILL -<pgid>`,
/// which BSD `kill` accepts and procps `kill` (the container's, where this suite is
/// authoritative) rejects as a malformed option, so inside the container it signalled
/// nothing. The direct child still died by `child.kill()`, but a grandchild holding the
/// pipe kept both readers waiting until it finished on its own. This test is what would
/// have said so: with the old spelling it takes the grandchild's full sixty seconds.
#[test]
fn a_bounded_run_that_times_out_takes_its_grandchildren_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("grandchild.pid");
    let started = std::time::Instant::now();
    let r = run_bounded(
        Command::new("sh")
            .arg("-c")
            .arg(format!("sleep 60 & echo $! > {}; wait", pid_file.display())),
        std::time::Duration::from_millis(500),
    );
    assert!(r.is_none(), "the deadline must be reported as a deadline");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "the readers waited {:?}: the grandchild outlived the deadline and held the pipe",
        started.elapsed()
    );
    let pid = std::fs::read_to_string(&pid_file).expect("the grandchild announced itself");
    let alive = Command::new("ps")
        .args(["-p", pid.trim(), "-o", "stat="])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            !s.is_empty() && !s.starts_with('Z')
        })
        .unwrap_or(false);
    assert!(!alive, "grandchild {} is still running", pid.trim());
}

/// How long any one stage invocation in this file gets.
const STAGE_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// Runs a **generated** compiler stage — `nzemit` and friends, compiled from `compiler/*.nz`
/// — under the same ceiling every other generated executable here runs under.
///
/// `Command::output()` gives neither an address-space ceiling nor a deadline. The
/// compiled *programs* in this file have always gone through `bounded_exe`; the stage
/// that compiles them had not, which is backwards: `nzemit` is one of the ten processes
/// that were running when the machine went down, and it is the one whose input is
/// attacker-shaped in the sense that matters here — whatever a test happens to write.
///
/// The Rust `nazm` binary and `clang` are deliberately *not* routed through this. They
/// are trusted build tools in the same class as `cargo`, not artefacts of the thing under
/// test.
///
/// Every stage is also handed the core prelude, which the checker and the emitter load as
/// the module keyed `@core/prelude` in every compilation — the toolchain's source,
/// `library/core/prelude.nz`, named on the command line because it is not the program's.
/// The lexer and parser drivers read one file and ignore the second argument.
fn run_stage(exe: &Path, arg: &Path) -> (Option<i32>, String, String) {
    let core = repo_root().join("library/core/prelude.nz");
    run_bounded(common::bounded_exe(exe).arg(arg).arg(&core), STAGE_LIMIT).unwrap_or_else(|| {
        panic!(
            "{} did not finish within {STAGE_LIMIT:?} on {}",
            exe.display(),
            arg.display()
        )
    })
}

fn build_emitter(dir: &Path) -> PathBuf {
    common::require_containment("emit.nz");
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

        let (rc, ir, ir_err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(0), "the Nazm compiler refused {expr:?}: {ir_err}");
        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir).expect("write IR");

        let exe = dir.path().join("case");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "clang rejected IR from the Nazm compiler for {expr:?}: {}\n{ir}",
            String::from_utf8_lossy(&built.stderr)
        );

        let (code, stdout, _) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(30),
        )
        .expect("the compiled program hung");
        assert_eq!(code, Some(0), "{expr:?}: {stdout}");
        assert_eq!(stdout.trim(), *expected, "{expr:?}");

        // And the interpreter agrees with the same expectation.
        let interp = Command::new(NAZM)
            .args(["run", "--interpret", file.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&interp.stdout).trim(),
            *expected,
            "the expectation disagrees with the interpreter for {expr:?}"
        );
    }
}

/// v1, Q1-B-05: the compiler written in Nazm holds `exit_with` to the same status contract as the
/// reference — a status in `0..=255` reaches the host unchanged, one outside fails with `N0416`
/// and status 2 — and the interpreter agrees with both.
#[test]
fn the_nazm_written_compiler_holds_exit_with_to_0_to_255() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    for (status, expected, failure) in [
        ("7", 7, false),
        ("255", 255, false),
        ("300", 2, true),
        ("0 - 1", 2, true),
    ] {
        let src = format!("fn main(io: IoCap) -> Int ! {{ io }} {{ exit_with({status}) }}\n");
        let file = dir.path().join("exit.nz");
        std::fs::write(&file, &src).expect("write");
        let (rc, ir, ir_err) = run_stage(&emitter, &file);
        assert_eq!(
            rc,
            Some(0),
            "the Nazm compiler refused exit_with({status}): {ir_err}"
        );
        let ll = dir.path().join("exit.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let exe = dir.path().join("exit");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let (code, _, stderr) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(30),
        )
        .expect("the compiled program hung");
        assert_eq!(code, Some(expected), "exit_with({status}): {stderr}");
        assert_eq!(
            stderr.contains("N0416"),
            failure,
            "exit_with({status}): {stderr}"
        );
        let interp = Command::new(NAZM)
            .args(["run", "--interpret", file.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            interp.status.code(),
            Some(expected),
            "the interpreter, exit_with({status})"
        );
    }
}

/// The compiler written in Nazm emits the **same memory semantics** as the one written in
/// Rust.
///
/// Not "it compiles and the answer is right": both programs are asked what they allocated
/// and what they reclaimed, and the two reports must be the same line. That is the
/// distinction N8 exists to make — for a milestone the Nazm-written compiler emitted a
/// runtime with no reference count at all, and every behavioural test still passed, because
/// a program that never frees anything computes the same answers as one that does.
///
/// The runtime *text* the two emit is held together by `cargo xtask check`'s `runtime
/// parity` gate. What that gate cannot check is where each emitter decides to retain and
/// release, which is what this test is for.
#[test]
fn the_nazm_written_compiler_emits_the_same_memory_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    // Each case: the program, and what it must print. The counts are not written down
    // here — they are compared between the two compilers, because an expectation written
    // by hand can be wrong in the same direction twice.
    let cases: &[(&str, &str)] = &[
        // A sequence created, left behind, and reclaimed.
        (
            "fn main() -> Int {\n    let xs = ints_new();\n    ints_push(xs, 7)\n}\n",
            "1",
        ),
        // Short-lived heap strings: the workload that used to grow without bound. Prepended, so
        // each step makes a string of its own: the reference appends `s = str_concat(s, t)` in
        // place since R1b (D2), and the Nazm-written compiler does not, so an append's counts differ.
        (
            "fn main() -> Int {\n    let mut s = \"\";\n    let mut i = 0;\n    \
             while i < 40 {\n        s = str_concat(\"x\", s);\n        i = i + 1;\n    }\n    \
             str_len(s)\n}\n",
            "40",
        ),
        // A slice outliving the binding of the buffer it points into.
        (
            "fn make() -> Str {\n    let s = str_concat(\"ab\", \"cd\");\n    \
             str_slice(s, 1, 3)\n}\n\
             fn main() -> Int {\n    let x = make();\n    let mut i = 0;\n    \
             let mut noise = \"\";\n    while i < 20 {\n        \
             noise = str_concat(\"zz\", noise);\n        i = i + 1;\n    }\n    \
             str_byte(x, 0)\n}\n",
            "98",
        ),
        // A string stored in a sequence, read back after its own binding was rebound,
        // and the sequence's contents released with the sequence.
        (
            "fn build() -> Int {\n    let xs = strs_new();\n    let mut i = 0;\n    \
             while i < 12 {\n        strs_push(xs, int_to_str(i));\n        i = i + 1;\n    }\n    \
             let got = strs_get(xs, 3);\n    strs_set(xs, 3, \"\");\n    str_byte(got, 0)\n}\n\
             fn main() -> Int { build() }\n",
            "51",
        ),
        // A borrowed string returned, on both edges.
        (
            "fn same(s: Str) -> Str { s }\n\
             fn same_returned(s: Str) -> Str { return s; }\n\
             fn main() -> Int {\n    let s = str_concat(\"ab\", \"cd\");\n    \
             str_len(same(s)) + str_len(same_returned(s))\n}\n",
            "8",
        ),
        // Reclamation on the failure path.
        (
            "fn main() -> Int {\n    let s = str_concat(\"ab\", \"cd\");\n    \
             let xs = ints_new();\n    ints_get(xs, 0)\n}\n",
            "",
        ),
        // Channels: created, used and destroyed, many times over.
        (
            "fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
             while i < 10 {\n        let c = chan_new(1);\n        chan_send(c, i);\n        \
             let buf = ints_new();\n        chan_recv(c, buf);\n        \
             total = total + ints_get(buf, 0);\n        i = i + 1;\n    }\n    total\n}\n",
            "45",
        ),
        // A channel shared with tasks, reclaimed after the scope has joined.
        (
            "fn feed(c: Chan, n: Int) -> Int { chan_send(c, n); 0 }\n\
             fn main(tasks: SpawnCap) -> Int {\n    let c = chan_new(4);\n    \
             scope {\n        spawn feed(c, 1);\n        spawn feed(c, 2);\n    }\n    \
             chan_close(c);\n    let buf = ints_new();\n    \
             while chan_recv(c, buf) {\n    }\n    ints_len(buf)\n}\n",
            "2",
        ),
        // Records: copied, nested, replaced field by field, and returned — all of it on
        // top of fields that own something, so a drop that did not recurse or a copy that
        // did not retain would show as a different count rather than a different answer.
        (
            "struct Inner {\n    text: Str,\n}\n\
             struct Outer {\n    inner: Inner,\n    values: Ints,\n}\n\
             fn make(n: Int) -> Outer {\n    \
             Outer(inner: Inner(text: int_to_str(n)), values: ints_new())\n}\n\
             fn main() -> Int {\n    let mut o = make(7);\n    let p = o;\n    \
             o.inner.text = str_concat(o.inner.text, \"!\");\n    \
             ints_push(p.values, 1);\n    let mut i = 0;\n    let mut total = 0;\n    \
             while i < 30 {\n        let each = make(i);\n        \
             total = total + str_len(each.inner.text);\n        i = i + 1;\n    }\n    \
             total + str_len(o.inner.text) + ints_len(p.values)\n}\n",
            "53",
        ),
        // A projection out of a temporary, which must take its own reference before the
        // record it came from is dropped.
        (
            "struct Holder {\n    name: Str,\n}\n\
             fn make() -> Holder { Holder(name: str_concat(\"ab\", \"cd\")) }\n\
             fn main() -> Int {\n    let s = make().name;\n    \
             let mut noise = \"\";\n    let mut i = 0;\n    \
             while i < 20 {\n        noise = str_concat(\"zz\", noise);\n        i = i + 1;\n    }\n    \
             str_byte(s, 3)\n}\n",
            "100",
        ),
        // A construction that fails part way through, and a temporary still in hand when
        // it does. Both compilers have to release both.
        (
            "struct Two {\n    a: Str,\n    b: Int,\n}\n\
             fn main() -> Int {\n    let z = 0;\n    \
             let t = Two(a: str_concat(\"ab\", \"cd\"), b: str_len(str_concat(\"x\", int_to_str(1 / z))));\n    \
             t.b\n}\n",
            "",
        ),
        // Enums: the active variant copied, matched, replaced across variants, and
        // returned out of the arm that matched it — with an owning variant that is never
        // the active one, so a compiler that copied or released every arm would show a
        // different count rather than a different answer.
        (
            "enum Holder {\n    Nothing,\n    Many(items: Strs),\n    Text(value: Str),\n}\n\
             fn take(h: Holder) -> Str { match h { Holder.Nothing() => \"\", \
             Holder.Many(items: _) => \"\", Holder.Text(value: s) => s, } }\n\
             fn main() -> Int {\n    let mut h = Holder.Text(value: str_concat(\"ab\", \"cd\"));\n    \
             let copy = h;\n    h = Holder.Nothing();\n    \
             h = Holder.Text(value: str_concat(\"x\", \"y\"));\n    let mut i = 0;\n    \
             let mut total = 0;\n    \
             while i < 30 {\n        \
             total = total + str_len(take(Holder.Text(value: int_to_str(i))));\n        \
             i = i + 1;\n    }\n    \
             total + str_len(take(copy)) + str_len(take(h))\n}\n",
            "56",
        ),
        // A nested enum inside a record, so both helpers have to recurse into the other.
        (
            "enum Inner {\n    None,\n    Text(value: Str),\n}\n\
             struct Event {\n    state: Inner,\n    values: Ints,\n}\n\
             fn size(e: Inner) -> Int { match e { Inner.None() => 0, \
             Inner.Text(value: s) => str_len(s), } }\n\
             fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
             while i < 30 {\n        \
             let e = Event(state: Inner.Text(value: int_to_str(i)), values: ints_new());\n        \
             let copy = e;\n        total = total + size(copy.state) + ints_len(e.values);\n        \
             i = i + 1;\n    }\n    total\n}\n",
            "50",
        ),
        // A record and an enum **crossing a task boundary**, which is where the reference
        // and this compiler diverged until N10 looked: the argument block's offsets were
        // arithmetic — twenty-four bytes for a `Str` and eight for everything else — so a
        // record was given eight and the task read the next argument's bytes as its own.
        // And the trampoline released four kinds of value and not these two, so what it
        // was given was never handed back. The answer was wrong *and* the string leaked.
        (
            "struct M {\n    text: Str,\n}\n\
             enum E {\n    Empty,\n    Text(value: Str),\n}\n\
             fn worker(m: M, e: E, out: Chan) -> Int {\n    \
             let n = str_len(m.text) + match e { E.Empty() => 0, \
             E.Text(value: s) => str_len(s), };\n    \
             if chan_send(out, n) { 1 } else { 0 }\n}\n\
             fn main(tasks: SpawnCap) -> Int {\n    let s = str_concat(\"he\", \"llo\");\n    \
             let out = chan_new(4);\n    let got = ints_new();\n    \
             scope {\n        spawn worker(M(text: s), E.Text(value: s), out);\n        \
             spawn worker(M(text: s), E.Text(value: s), out);\n    }\n    \
             if chan_recv(out, got) { 0 } else { 0 };\n    \
             if chan_recv(out, got) { 0 } else { 0 };\n    \
             ints_get(got, 0) + ints_get(got, 1)\n}\n",
            "20",
        ),
        // Leaving an expression that never finishes — the other three ways out of one.
        // The failure path has given back what a half-evaluated expression was holding
        // since N9; `break`, `continue` and `return` gave back nothing, in both backends
        // and since N8. A record abandoned part way, an enum abandoned part way, and an
        // arm that leaves the loop before the `match` reaches its join.
        (
            "struct R {\n    a: Str,\n    b: Int,\n}\n\
             enum E {\n    Absent,\n    Value(a: Str, b: Int),\n}\n\
             fn main() -> Int {\n    let mut i = 0;\n    let mut total = 0;\n    \
             while i < 4 {\n        i = i + 1;\n        \
             let r = R(a: int_to_str(i), b: if i == 1 { continue; } else { i });\n        \
             let v = E.Value(a: int_to_str(i), b: if i == 2 { continue; } else { r.b });\n        \
             total = total + match v { E.Absent() => 0, \
             E.Value(a: s, b: n) => if i == 3 { continue; } else { n }, };\n    }\n    \
             total\n}\n",
            "4",
        ),
        // N11: generic records, enums and functions, and `Vec[T]` of every kind of element
        // — `Int`, `Str`, a record owning a string, an enum with a string payload, and a
        // `Vec` — through push, get, set, pop and destruction. The instances are the
        // Nazm compiler's own, and what they reclaim must be the reference's.
        ("struct Pair[A, B] { first: A, second: B, }\nenum Maybe[T] { None, Some(value: T), }\nfn identity[T](value: T) -> T { value }\nfn or[T](m: Maybe[T], d: T) -> T { match m { Maybe.None() => d, Maybe.Some(value: v) => v, } }\nfn main() -> Int {\n    let xs = vec_new[Pair[Int, Str]]();\n    vec_push(xs, Pair[Int, Str](first: 7, second: \"nazm\"));\n    let p = vec_get(xs, 0);\n    let m = Maybe[Int].Some(value: 42);\n    identity[Int](p.first) + identity(str_len(p.second)) + or(m, 0) + or(Maybe[Int].None(), 1)\n}\n", "54"),
        ("struct Named { name: Str, n: Int, }\nenum Tok { Word(text: Str), Num(v: Int), End, }\nfn first[T](xs: Vec[T]) -> T { vec_get(xs, 0) }\nfn keep[T](xs: Vec[T], x: T) -> Int { vec_push(xs, x) }\nfn score(t: Tok) -> Int { match t { Tok.Word(text: s) => str_len(s), Tok.Num(v: v) => v, Tok.End() => 0, } }\nfn main() -> Int {\n    let names = vec_new[Named]();\n    let toks = vec_new[Tok]();\n    let strs = vec_new[Str]();\n    let nest = vec_new[Vec[Int]]();\n    let mut i = 0;\n    while i < 50 {\n        keep(names, Named(name: str_concat(\"n\", int_to_str(i)), n: i));\n        keep(toks, Tok.Word(text: int_to_str(i * 7)));\n        keep(toks, Tok.Num(v: i));\n        vec_push(strs, str_concat(\"s\", int_to_str(i)));\n        let inner = vec_new[Int]();\n        vec_push(inner, i);\n        vec_push(nest, inner);\n        i = i + 1;\n    }\n    vec_set(names, 3, Named(name: \"replaced\", n: 1000));\n    vec_set(strs, 0, vec_get(strs, 1));\n    let popped = vec_pop(toks);\n    let mut total = 0;\n    let mut k = 0;\n    while k < vec_len(toks) { total = total + score(vec_get(toks, k)); k = k + 1; }\n    let p = first(names);\n    total + vec_get(names, 3).n + str_len(first(strs)) + score(popped) + str_len(p.name)\n        + vec_get(vec_get(nest, 9), 0) + vec_len(nest)\n}\n", "2421"),
        ("struct Pair[A, B] { first: A, second: B, }\nenum Maybe[T] { None, Some(value: T), }\nfn swap[A, B](p: Pair[A, B]) -> Pair[B, A] { Pair[B, A](first: p.second, second: p.first) }\nfn get_or[T](m: Maybe[T], d: T) -> T { match m { Maybe.None() => d, Maybe.Some(value: v) => v, } }\nfn wrap[T](x: T) -> Maybe[T] { Maybe[T].Some(value: x) }\nfn tag[T](x: T, s: Str) -> Str { str_concat(s, \"!\") }\nfn main() -> Int {\n    let p = swap(Pair[Int, Str](first: 1, second: \"abc\"));\n    let q = swap(p);\n    let m = wrap(str_concat(\"x\", \"yz\"));\n    let s = get_or(m, \"none\");\n    let n = get_or(wrap(5), 0) + get_or(Maybe[Int].None(), 7);\n    str_len(p.first) + q.first + str_len(s) + n + str_len(tag(3, \"hi\")) + str_len(tag(\"a\", \"hey\"))\n}\n", "26"),
        ("fn id[T](x: T) -> T { x }\nfn twice[T](x: T) -> Vec[T] { let v = vec_new[T](); vec_push(v, id(x)); vec_push(v, x); v }\nfn main() -> Int {\n    let a = twice(3);\n    let b = twice(\"q\");\n    let c = twice(a);\n    vec_len(a) + vec_len(b) + vec_len(c) + vec_get(vec_get(c, 1), 0) + str_len(vec_pop(b))\n}\n", "10"),
        // An unbound read still in hand while the element it came from is replaced: only
        // `vec_get`'s own copy keeps it valid, and the callee's noise would reuse the block.
        ("fn first_byte_later(s: Str, n: Int) -> Int {\n    let noise = vec_new[Str]();\n    let mut i = 0;\n    while i < 64 { vec_push(noise, str_concat(\"zz\", \"zz\")); i = i + 1; }\n    str_byte(s, 0) + n * 0\n}\nfn main() -> Int {\n    let xs = vec_new[Str]();\n    vec_push(xs, str_concat(\"ab\", \"cd\"));\n    first_byte_later(vec_get(xs, 0), vec_set(xs, 0, int_to_str(7)))\n}\n", "97"),
        // A failure inside a generic built-in's bounds check still gives back everything.
        ("fn main() -> Int { let xs = vec_new[Str](); vec_push(xs, \"a\"); str_len(vec_get(xs, 3)) }\n", ""),
    ];

    same_memory_semantics(dir.path(), &emitter, cases);
}

/// Compile each case with the Nazm-written compiler and with the reference, run both, and
/// require the same exit status, the same value — the one given — and the same memory
/// report, with nothing left live.
fn same_memory_semantics(dir: &Path, emitter: &Path, cases: &[(&str, &str)]) {
    /// The `nazm-memory:` line, which both compilers' programs print under the same
    /// variable because both take its text from one place.
    fn report(stderr: &str) -> String {
        stderr
            .lines()
            .find(|l| l.starts_with("nazm-memory:"))
            .unwrap_or_else(|| panic!("no memory report in: {stderr}"))
            .to_owned()
    }

    for (source, expected) in cases {
        let file = dir.join("mem.nz");
        std::fs::write(&file, source).expect("write");

        // The Nazm-written compiler: IR out, clang in, then run it.
        let (rc, ir, ir_err) = run_stage(emitter, &file);
        assert_eq!(
            rc,
            Some(0),
            "the Nazm compiler refused:\n{source}\n{ir_err}"
        );
        let ll = dir.join("mem.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let theirs = dir.join("mem-selfhost");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-lpthread", "-o"])
            .arg(&theirs)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "clang rejected IR from the Nazm compiler:\n{}\n{ir}",
            String::from_utf8_lossy(&built.stderr)
        );

        // The reference compiler, from the same source.
        let ours = dir.join("mem-reference");
        let out = Command::new(NAZM)
            .args(["build", file.to_str().unwrap(), "-o"])
            .arg(&ours)
            .output()
            .expect("run nazm build");
        assert!(
            out.status.success(),
            "the reference compiler refused:\n{source}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let run = |exe: &Path| {
            run_bounded(
                common::bounded_exe(exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                std::time::Duration::from_secs(60),
            )
            .expect("the compiled program hung")
        };
        let (their_code, their_out, their_err) = run(&theirs);
        let (our_code, our_out, our_err) = run(&ours);

        assert_eq!(their_code, our_code, "exit status differs for:\n{source}");
        assert_eq!(
            their_out.trim(),
            our_out.trim(),
            "value differs for:\n{source}"
        );
        assert_eq!(their_out.trim(), *expected, "{source}");
        assert_eq!(
            report(&their_err),
            report(&our_err),
            "the two compilers' programs disagree about what they reclaimed:\n{source}"
        );
        // And what they reclaimed is everything: a report that matched because both
        // leaked would pass the comparison above and fail here.
        let line = report(&their_err);
        for class in ["sequences", "strings", "channels", "closures"] {
            let live = line
                .split_whitespace()
                .skip_while(|w| *w != class)
                .nth(3)
                .unwrap_or("live=?");
            assert_eq!(live, "live=0", "{class} left behind:\n{line}\n{source}");
        }
    }
}

/// The text of an operand position that holds a value nobody produced.
///
/// Not every `void`: `call void @nz.release(…)`, `ret void` and `define void` are what a
/// runtime helper legitimately is. What is never legitimate is a `void` **with a value
/// after it** — an argument, a `phi`, a `store`, a `ret` of one — or a slot of that type,
/// and those are the forms the Nazm-written emitter produced for a transfer in a value
/// position until N10.1: `call i64 @nz.f0({ ptr, i64, ptr } %19, void %24)`.
fn fake_operand(ir: &str) -> Option<&str> {
    ir.lines().find(|l| {
        l.contains("void %")
            || l.contains("alloca void")
            || l.contains("phi void")
            || l.contains("store void")
    })
}

#[test]
fn a_transfer_in_a_value_position_is_never_a_value() {
    // N10.1. `break`, `continue` and `return` may sit inside anything used as a value — an
    // argument, an operand, an initialiser, a condition, a scrutinee, an arm — and when
    // one runs, the enclosing expression has no value on that path and never will. The
    // compiler written in Nazm got this wrong in two places that agreed with each other:
    // its checker typed `if c { continue; } else { i }` by the branch that left, so the
    // `if` had no value, and its emitter opened every join whether or not a branch
    // reached it, so a construct whose branches all left handed its parent an empty
    // stack. The first passed `void %24` to a call; the second popped someone else's
    // value, or failed with `N0405` inside the compiler.
    //
    // Each case in `tests/transfers/` is run **five** ways, and all five must agree: the
    // interpreter, the reference native backend, and the Nazm-written compiler both
    // compiled and interpreted — whose IR must be byte-identical, because they are one
    // program — and the executable that IR assembles to. The value and the exit status
    // are written down by hand in `.expected`, so an answer all five share can still be
    // wrong; the reclamation counts are compared rather than written, and must also be
    // zero live.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let corpus = repo_root().join("crates/nazm-cli/tests/transfers");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .expect("read the transfer corpus")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "nz"))
        .collect();
    cases.sort();
    // A corpus that quietly lost its files would pass every assertion below.
    assert!(cases.len() >= 32, "only {} transfer cases", cases.len());

    fn memory_line(stderr: &str) -> String {
        stderr
            .lines()
            .find(|l| l.starts_with("nazm-memory:"))
            .unwrap_or_else(|| panic!("no memory report in: {stderr}"))
            .to_owned()
    }
    fn failure_code(stderr: &str) -> Option<String> {
        let at = stderr.find("error[")?;
        stderr.get(at + 6..at + 11).map(ToOwned::to_owned)
    }

    for file in &cases {
        let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let expected = std::fs::read_to_string(file.with_extension("expected"))
            .expect("every case has an .expected");
        let expected = expected.trim();
        let wants_failure = expected.strip_prefix("fails ");

        // The interpreter: the semantic oracle this backend is compared against.
        let interp = Command::new(NAZM)
            .args(["run", "--interpret", file.to_str().unwrap()])
            .env(nazm_lir::MEMORY_VARIABLE, "1")
            .output()
            .expect("run nazm run");
        let interp_err = String::from_utf8_lossy(&interp.stderr).into_owned();

        // The reference native backend.
        let ours = dir.path().join(format!("{name}-reference"));
        let built = Command::new(NAZM)
            .args(["build", file.to_str().unwrap(), "-o"])
            .arg(&ours)
            .output()
            .expect("run nazm build");
        assert!(
            built.status.success(),
            "{name}: the reference compiler refused: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        // The Nazm-written compiler, compiled.
        let (rc, ir, ir_err) = run_stage(&emitter, file);
        assert_eq!(rc, Some(0), "{name}: the Nazm compiler refused:\n{ir_err}");
        assert!(
            fake_operand(&ir).is_none(),
            "{name}: a value nobody produced reached an operand: {:?}",
            fake_operand(&ir)
        );

        // The same compiler, interpreted. One program in two execution forms, so the
        // output is one output — a difference is a defect in whichever form is wrong.
        let interpreted = Command::new(NAZM)
            .args([
                "run",
                "--interpret",
                "compiler/emit.nz",
                "--",
                file.to_str().unwrap(),
                "library/core/prelude.nz",
            ])
            .current_dir(repo_root())
            .output()
            .expect("interpret the Nazm compiler");
        assert!(
            interpreted.status.success(),
            "{name}: the interpreted Nazm compiler failed: {}",
            String::from_utf8_lossy(&interpreted.stderr)
        );
        assert!(
            String::from_utf8_lossy(&interpreted.stdout) == ir,
            "{name}: the Nazm compiler emits different IR interpreted and compiled"
        );

        let ll = dir.path().join(format!("{name}.ll"));
        std::fs::write(&ll, &ir).expect("write IR");
        let theirs = dir.path().join(format!("{name}-selfhost"));
        let assembled = Command::new("clang")
            .args(["-Wno-override-module", "-lpthread", "-o"])
            .arg(&theirs)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            assembled.status.success(),
            "{name}: clang rejected IR from the Nazm compiler:\n{}",
            String::from_utf8_lossy(&assembled.stderr)
        );

        let run = |exe: &Path| {
            run_bounded(
                common::bounded_exe(exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                std::time::Duration::from_secs(60),
            )
            .expect("the compiled program hung")
        };
        let (ref_code, ref_out, ref_err) = run(&ours);
        let (sh_code, sh_out, sh_err) = run(&theirs);

        match wants_failure {
            Some(code) => {
                for (who, status, err) in [
                    ("interpreter", interp.status.code(), interp_err.as_str()),
                    ("reference", ref_code, ref_err.as_str()),
                    ("selfhost", sh_code, sh_err.as_str()),
                ] {
                    assert_eq!(status, Some(2), "{name}: {who} did not fail:\n{err}");
                    assert_eq!(
                        failure_code(err).as_deref(),
                        Some(code),
                        "{name}: {who} failed differently:\n{err}"
                    );
                }
            }
            None => {
                assert_eq!(
                    interp.status.code(),
                    Some(0),
                    "{name}: interpreter: {interp_err}"
                );
                assert_eq!(ref_code, Some(0), "{name}: reference: {ref_err}");
                assert_eq!(sh_code, Some(0), "{name}: selfhost: {sh_err}");
                let interp_out = String::from_utf8_lossy(&interp.stdout).into_owned();
                for (who, out) in [
                    ("interpreter", interp_out.as_str()),
                    ("reference", ref_out.as_str()),
                    ("selfhost", sh_out.as_str()),
                ] {
                    assert_eq!(out.trim(), expected, "{name}: {who} gave the wrong answer");
                }
            }
        }

        // Reclamation, on the success paths and the failing ones: what was produced
        // before a transfer is given back exactly once, in every form.
        let line = memory_line(&interp_err);
        assert_eq!(
            memory_line(&ref_err),
            line,
            "{name}: reference reclaimed differently"
        );
        assert_eq!(
            memory_line(&sh_err),
            line,
            "{name}: selfhost reclaimed differently"
        );
        for class in ["sequences", "strings", "channels"] {
            let live = line
                .split_whitespace()
                .skip_while(|w| *w != class)
                .nth(3)
                .unwrap_or("live=?");
            assert_eq!(live, "live=0", "{name}: {class} left behind:\n{line}");
        }
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
        let (rc, ir, ir_err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(0), "the Nazm compiler refused a program: {ir_err}");
        let text = &ir;
        // Symbols are generated from the index, never from the source name: a Nazm
        // identifier would need escaping rules to be safe in an LLVM identifier, and the
        // escaping is the bug.
        assert!(text.contains("@nz.f"), "no generated symbols:\n{text}");
        assert!(
            !text.contains("@double") && !text.contains("@helper"),
            "a source name reached an LLVM identifier:\n{text}"
        );

        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir).expect("write");
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
        let (code, stdout, _) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(30),
        )
        .expect("hung");
        assert_eq!(code, Some(0));
        assert_eq!(stdout.trim(), *expected);

        let interp = Command::new(NAZM)
            .args(["run", "--interpret", file.to_str().unwrap()])
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
        let (rc, ir, _) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(0), "{expr:?}");
        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir).expect("write");
        let exe = dir.path().join("case");
        assert!(Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("clang")
            .status
            .success());

        let (status, stdout, stderr) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(30),
        )
        .expect("hung");
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
        assert!(!ir.contains("nsw"), "{expr:?}: emitted `nsw`:\n{ir}");
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
        // An enum declared inside a function is refused in the parser, so the refusal
        // arrives before emission is reached at all. (A bare block was this case until
        // N12.1, when both compilers came to compile one.)
        (
            "fn main() -> Int { enum E { A } 0 }\n",
            "not implemented yet",
        ),
        // A program that does not check is a *check* failure, reported with the
        // checker's own code — never as a backend limit.
        ("fn main() -> Int { 1 + true }\n", "N0304"),
        ("fn main() -> Int { undefined_name }\n", "N0200"),
    ] {
        let file = dir.path().join("bad.nz");
        std::fs::write(&file, src).expect("write");
        let (rc, ir, err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(1), "{src:?} should be refused");
        assert!(
            err.contains(expected),
            "{src:?}: expected {expected}: {err}"
        );
        assert!(ir.is_empty(), "{src:?}: emitted IR for a refused program");
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
    // Under a deadline, as every stage here is: a stage that never finishes — a compiler
    // miscompiled into a loop — is a failed bootstrap, not a suite that waits forever.
    const BOOTSTRAP_LIMIT: std::time::Duration = std::time::Duration::from_secs(900);
    let (status, stdout, stderr) = run_bounded(
        Command::new("sh").arg(&script).arg(&out).current_dir(&root),
        BOOTSTRAP_LIMIT,
    )
    .unwrap_or_else(|| panic!("bootstrap.sh did not finish within {BOOTSTRAP_LIMIT:?}"));
    assert_eq!(status, Some(0), "the bootstrap failed:\n{stdout}\n{stderr}");

    let record = std::fs::read_to_string(out.join("record.txt")).expect("the record");
    for expected in ["ir: identical", "executables: byte-identical"] {
        assert!(
            record.contains(expected),
            "the record does not say {expected:?}:\n{record}"
        );
    }
    // Whole lines, and a floor rather than an exact count.
    //
    // This read `record.contains("conformance-cases: 5")` until N9, and it was passing
    // for the wrong reason: there were ten cases, and the substring it looked for was the
    // tail of `module-conformance-cases: 5`. A corpus that had shrunk to five would have
    // satisfied it, and so would one that had shrunk to none while the module count
    // happened to be five. Parsing the number is what makes it an assertion about the
    // corpus rather than about the text.
    let count = |key: &str| -> usize {
        record
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    // N11 added two generic programs, two generic module cases and four refusals; N12 a
    // propagation program, a module exporting a `Result` API and five refusals; N12.1 a
    // value-block program, a module whose importer uses arm blocks and three refusals.
    assert!(
        count("conformance-cases:") >= 28,
        "the corpus has shrunk:\n{record}"
    );
    assert!(
        count("module-conformance-cases:") >= 11,
        "the module corpus has shrunk:\n{record}"
    );
    assert!(
        count("refusal-conformance-cases:") >= 12,
        "the refusal corpus has shrunk:\n{record}"
    );
    // Every stage loaded the same core prelude, and the record says which bytes it was.
    assert!(
        record
            .lines()
            .any(|l| l.starts_with("core-prelude: library/core/prelude.nz ")),
        "the record does not name the core prelude:\n{record}"
    );
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

/// The compiler written in Nazm resolves `use` itself.
///
/// Until this existed the *driver* read the import graph and handed the compiler one
/// merged buffer, so "the compiler written in Nazm" could not build a multi-file program
/// on its own — the release report listed it under *Known limitations*. The program below
/// is arranged so that a resolver which merely concatenated would fail: `lib/math.nz` is
/// reached twice, once as `./lib/math.nz` from the root and once as `math.nz` from inside
/// `lib/`, and the two spellings must fold to one file or `triple` is defined twice and
/// nothing compiles.
#[test]
fn the_nazm_written_compiler_resolves_its_own_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    let root = dir.path();
    std::fs::create_dir_all(root.join("lib")).expect("lib");
    // `pub` since N2, 2026-09-22: both of these are called from another module, which is
    // now something a module has to say. The program is otherwise the one it always was.
    std::fs::write(
        root.join("lib/math.nz"),
        "pub fn triple(n: Int) -> Int { n * 3 }\n",
    )
    .expect("math");
    std::fs::write(
        root.join("lib/util.nz"),
        "use \"math.nz\";\npub fn twice_tripled(n: Int) -> Int { triple(n) + triple(n) }\n",
    )
    .expect("util");
    let prog = root.join("prog.nz");
    std::fs::write(
        &prog,
        "use \"lib/util.nz\";\nuse \"./lib/math.nz\";\nfn main() -> Int { twice_tripled(7) + triple(1) }\n",
    )
    .expect("prog");

    let (rc, ir, ir_err) = run_stage(&emitter, &prog);
    assert_eq!(
        rc,
        Some(0),
        "the Nazm compiler could not resolve the imports: {ir_err}"
    );
    let ll = root.join("prog.ll");
    std::fs::write(&ll, &ir).expect("write IR");

    let exe = root.join("prog");
    let built = Command::new("clang")
        .args(["-Wno-override-module", "-o"])
        .arg(&exe)
        .arg(&ll)
        .output()
        .expect("run clang");
    assert!(
        built.status.success(),
        "clang rejected the IR: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let (code, stdout, _) = run_bounded(
        &mut common::bounded_exe(&exe),
        std::time::Duration::from_secs(30),
    )
    .expect("the compiled program hung");
    assert_eq!(code, Some(0), "{stdout}");
    assert_eq!(
        stdout.trim(),
        "45",
        "twice_tripled(7) + triple(1) is 45; a resolver that dropped an import or read one \
         file twice would not get here at all"
    );

    // The reference implementations, which resolve imports their own way, agree.
    let reference = root.join("prog_rs");
    let rs = Command::new(NAZM)
        .args(["build", prog.to_str().unwrap()])
        .arg("-o")
        .arg(&reference)
        .output()
        .expect("nazm build");
    assert!(
        rs.status.success(),
        "the reference compiler rejected it: {}",
        String::from_utf8_lossy(&rs.stderr)
    );
    let (_, rs_out, _) = run_bounded(
        &mut common::bounded_exe(&reference),
        std::time::Duration::from_secs(30),
    )
    .expect("hung");
    assert_eq!(rs_out.trim(), "45", "the reference compiler disagrees");

    let interp = Command::new(NAZM)
        .args(["run", "--interpret", prog.to_str().unwrap()])
        .output()
        .expect("nazm run");
    assert_eq!(
        String::from_utf8_lossy(&interp.stdout).trim(),
        "45",
        "the interpreter disagrees"
    );
}

/// An import that is not there is refused by name, rather than quietly producing a
/// program missing the functions it needed.
#[test]
fn the_nazm_written_compiler_refuses_an_import_it_cannot_find() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let prog = dir.path().join("bad.nz");
    std::fs::write(&prog, "use \"nowhere.nz\";\nfn main() -> Int { 0 }\n").expect("write");

    let (rc, _, err) = run_stage(&emitter, &prog);
    assert_eq!(rc, Some(2), "a missing import must fail, got {rc:?}");
    let said = err;
    assert!(
        said.contains("cannot find"),
        "it must name what it could not find, got {said:?}"
    );
}

/// N77, N102: a package's module is refused by name, not looked for as a file and reported
/// missing. A standard module resolves since N102, from the library beside the core prelude —
/// and one that is not there is "cannot find", as a missing file is.
#[test]
fn the_nazm_written_compiler_resolves_std_and_refuses_package_imports_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let prog = dir.path().join("bad.nz");
    std::fs::write(&prog, "use \"dep:util.nz\";\nfn main() -> Int { 0 }\n").expect("write");
    let (rc, _, err) = run_stage(&emitter, &prog);
    assert_eq!(rc, Some(2), "{err}");
    assert!(
        err.contains("a module of another package") && err.contains("dep:util.nz"),
        "{err}"
    );
    assert!(!err.contains("cannot find"), "{err}");

    std::fs::write(&prog, "use \"@std/nowhere\";\nfn main() -> Int { 0 }\n").expect("write");
    let (rc, _, err) = run_stage(&emitter, &prog);
    assert_eq!(rc, Some(2), "{err}");
    assert!(err.contains("cannot find"), "{err}");

    // Two modules naming one standard module share it: one key, one copy of its functions.
    let good = dir.path().join("good.nz");
    std::fs::write(
        dir.path().join("half.nz"),
        "use \"@std/num\";\npub fn half(n: Int) -> Int { num_max(n / 2, 1) }\n",
    )
    .expect("write");
    std::fs::write(
        &good,
        "use \"@std/num\";\nuse \"half.nz\";\nfn main() -> Int { num_abs(0 - 9) * 10 + half(6) + num_min(4, 2) }\n",
    )
    .expect("write");
    let (rc, ir, err) = run_stage(&emitter, &good);
    assert_eq!(rc, Some(0), "{err}");
    let ll = dir.path().join("good.ll");
    std::fs::write(&ll, &ir).expect("write IR");
    let exe = dir.path().join("good");
    let built = Command::new("clang")
        .args(["-Wno-override-module", "-o"])
        .arg(&exe)
        .arg(&ll)
        .output()
        .expect("run clang");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let got = run_native(&exe);
    assert_eq!(got, (Some(0), "95\n".to_owned(), None));
}

/// The compiler written in Nazm compiles a concurrent program.
///
/// It could parse and check one before this; it could not lower one. The programs below
/// go all the way through *it* — not the reference — and are compared against the
/// reference's answer for the same source.
#[test]
fn the_nazm_written_compiler_compiles_concurrency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let root = repo_root();

    let pool = std::fs::read_to_string(root.join("examples/worker-pool.nz")).expect("read");
    let cases: &[(&str, &str, &str)] = &[
        (
            "two tasks and a channel",
            "fn w(c: Chan) -> Int { chan_send(c, 5); 0 }\n\
             fn main(tasks: SpawnCap) -> Int {\n\
             let c = chan_new(2);\n\
             let buf = ints_new();\n\
             let mut t = 0;\n\
             scope { spawn w(c); spawn w(c); chan_recv(c, buf); t = t + ints_pop(buf); \
             chan_recv(c, buf); t = t + ints_pop(buf); }\n\
             t\n\
             }\n",
            "10",
        ),
        // It prints 90 and returns 0 (R1, 5b.3), which the result report adds.
        ("the specification's worker pool", &pool, "90\n0"),
    ];

    for (what, src, expected) in cases {
        let file = dir.path().join("case.nz");
        std::fs::write(&file, src).expect("write");

        let (rc, ir, ir_err) = run_stage(&emitter, &file);
        assert_eq!(
            rc,
            Some(0),
            "{what}: the Nazm compiler refused it: {ir_err}"
        );
        let ll = dir.path().join("case.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let exe = dir.path().join("case");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-pthread", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "{what}: clang rejected the IR: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        let (code, stdout, _) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(60),
        )
        .unwrap_or_else(|| panic!("{what}: the compiled program hung"));
        assert_eq!(code, Some(0), "{what}: {stdout}");
        assert_eq!(stdout.trim(), *expected, "{what}");

        let interp = Command::new(NAZM)
            .args(["run", "--interpret", file.to_str().unwrap()])
            .output()
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&interp.stdout).trim(),
            *expected,
            "{what}: the interpreter disagrees"
        );
    }
}

/// And its programs report a failure the way the specification says: at the closing brace,
/// after the scope's tasks have been joined.
///
/// The worker is ordered by channel rather than by a delay — it announces itself on
/// `started` and blocks on `go`, so it is provably running and provably unfinished when
/// the failure happens.
#[test]
fn a_program_from_the_nazm_compiler_joins_its_scope_before_reporting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    const WORKER: &str = "fn worker(started: Chan, go: Chan, out: OutCap) -> Int {\n\
         let buf = ints_new();\n\
         chan_send(started, 1);\n\
         chan_recv(go, buf);\n\
         ints_pop(buf);\n\
         print(\"worker finished\");\n\
         0\n\
         }\n";

    let cases: &[(&str, String, &str)] = &[
        (
            "the body fails",
            format!(
                "{WORKER}fn main(io: IoCap, tasks: SpawnCap) -> Int {{\n\
                 let started = chan_new(1);\n\
                 let go = chan_new(1);\n\
                 let buf = ints_new();\n\
                 let mut top = 1;\n\
                 scope {{\n\
                 spawn worker(started, go, io);\n\
                 chan_recv(started, buf);\n\
                 ints_pop(buf);\n\
                 chan_send(go, 1);\n\
                 let mut n = 0;\n\
                 while n < 64 {{ top = top + top; n = n + 1; }}\n\
                 }}\n\
                 top\n\
                 }}\n"
            ),
            "N0400",
        ),
        (
            "a task fails",
            format!(
                "{WORKER}fn bad(dead: Chan) -> Int {{\n\
                 let buf = ints_new();\n\
                 chan_recv(dead, buf);\n\
                 1 / ints_pop(buf)\n\
                 }}\n\
                 fn main(io: IoCap, tasks: SpawnCap) -> Int {{\n\
                 let started = chan_new(1);\n\
                 let go = chan_new(1);\n\
                 let dead = chan_new(1);\n\
                 let buf = ints_new();\n\
                 scope {{\n\
                 spawn worker(started, go, io);\n\
                 spawn bad(dead);\n\
                 chan_recv(started, buf);\n\
                 ints_pop(buf);\n\
                 chan_send(go, 1);\n\
                 chan_send(dead, 0);\n\
                 }}\n\
                 0\n\
                 }}\n"
            ),
            "N0401",
        ),
    ];

    for (what, src, code) in cases {
        let file = dir.path().join("fail.nz");
        std::fs::write(&file, src).expect("write");
        let (emitted, ir, ir_err) = run_stage(&emitter, &file);
        assert_eq!(emitted, Some(0), "{what}: {ir_err}");
        let ll = dir.path().join("fail.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let exe = dir.path().join("fail");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-pthread", "-o"])
            .arg(&exe)
            .arg(&ll)
            .output()
            .expect("clang");
        assert!(
            built.status.success(),
            "{what}: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        let (rc, stdout, stderr) = run_bounded(
            &mut common::bounded_exe(&exe),
            std::time::Duration::from_secs(60),
        )
        .unwrap_or_else(|| panic!("{what}: hung"));
        assert_eq!(rc, Some(2), "{what}: {stderr}");
        assert!(
            stderr.contains(code),
            "{what}: expected {code}, got {stderr}"
        );
        assert!(
            stdout.contains("worker finished"),
            "{what}: the scope reported before its task finished — the marker is missing \
             from stdout, which is what a failure that did not join looks like: {stdout:?}"
        );
    }
}

#[test]
fn the_nazm_written_compiler_refuses_what_monomorphisation_cannot_do() {
    // N11. The native limits on instantiation, refused by both compilers with the same code
    // at the same position: an instance family that never ends (`N0357`), one past the
    // stated nesting bound (`N0358`). Ordinary recursion through a generic function was a
    // third case until N49, when the reference began to compile it; the Nazm-written
    // compiler compiles it since N102, as the test after this one holds.
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let mut nested = String::from("struct Box[T] { v: T, }\n");
    for i in 0..70 {
        nested.push_str(&format!(
            "fn f{i}[T](x: T) -> Int {{ f{}(Box[T](v: x)) }}\n",
            i + 1
        ));
    }
    nested.push_str("fn f70[T](x: T) -> Int { 0 }\nfn main() -> Int { f0(1) }\n");
    let cases: Vec<(String, &str)> = vec![
        (
            "fn grow[T](x: T, n: Int) -> Int {\n    if n == 0 { 0 } else {\n        \
             let v = vec_new[T]();\n        vec_push(v, x);\n        grow(v, n - 1) + 1\n    }\n}\n\
             fn main() -> Int { grow(1, 3) }\n"
                .to_owned(),
            "N0357",
        ),
        (nested, "N0358"),
    ];
    for (source, code) in &cases {
        let file = dir.path().join("m.nz");
        std::fs::write(&file, source).expect("write");
        let (rc, _, err) = run_stage(&emitter, &file);
        assert_eq!(rc, Some(1), "the Nazm compiler accepted:\n{source}");
        let out = Command::new(NAZM)
            .args(["build", file.to_str().unwrap(), "-o"])
            .arg(dir.path().join("prog"))
            .output()
            .expect("run nazm build");
        let reference = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(&format!("error[{code}]")), "{code}: {err}");
        assert!(
            reference.contains(&format!("error[{code}]")),
            "{code}: {reference}"
        );
        // The same position: the Nazm compiler writes `path:line:column`, and the reference
        // renders the same `line:column` after the path.
        let at = err
            .lines()
            .next()
            .and_then(|l| l.rsplit(" at ").next())
            .map(|p| p.split(':').skip(1).take(2).collect::<Vec<_>>().join(":"))
            .unwrap_or_default();
        assert!(
            reference.contains(&format!("m.nz:{}", at.trim())),
            "{code}: {err} / {reference}"
        );
    }
}

/// The compiler written in Nazm compiles recursion as the reference does (N102).
///
/// Until N102 it refused every call cycle by name, because N49's stack contract — a check
/// MIR places first in every function in a call cycle — had no counterpart in its emitter.
/// It now emits the reference runtime's `nz.stack_check` on entry to every function in a
/// cycle, so recursion compiles and an unbounded one fails with `N0408` at the function
/// entered (`docs/spec.md`, *Native recursion*). This test holds both halves on six shapes:
/// a function calling itself, a recursive function nothing calls, two functions calling
/// each other, a cycle through a `spawn` (which closes no cycle: a task has its own stack),
/// a cycle that only exists across two files, and a recursion with no bottom — each built
/// by both compilers, run, and compared on status, output and failure code, with the value
/// written down here as well, because two compilers can share a bug.
#[test]
fn the_nazm_written_compiler_compiles_recursion_as_the_reference_does() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    struct Cycle {
        what: &'static str,
        root: &'static str,
        /// Files beside the root, written before the case runs and removed after.
        extra: &'static [(&'static str, &'static str)],
        /// The exit status and output both executables must give, or the failure code.
        status: i32,
        expect: &'static str,
    }

    let cases: &[Cycle] = &[
        Cycle {
            what: "a function calling itself",
            root: "fn fact(n: Int) -> Int {\n    if n <= 1 { 1 } else { n * fact(n - 1) }\n}\nfn main() -> Int { fact(5) }\n",
            extra: &[],
            status: 0,
            expect: "120",
        },
        Cycle {
            what: "a recursive function nothing calls",
            root: "fn unused(n: Int) -> Int {\n    if n <= 0 { 0 } else { unused(n - 1) }\n}\n\
                   fn main() -> Int { 42 }\n",
            extra: &[],
            status: 0,
            expect: "42",
        },
        Cycle {
            what: "two functions calling each other",
            root: "fn is_even(n: Int) -> Int {\n    if n == 0 { 1 } else { is_odd(n - 1) }\n}\n\
             fn is_odd(n: Int) -> Int {\n    if n == 0 { 0 } else { is_even(n - 1) }\n}\n\
             fn main() -> Int { is_even(8) }\n",
            extra: &[],
            status: 0,
            expect: "1",
        },
        Cycle {
            what: "a cycle through a spawned task",
            root: "fn w(sp: SpawnCap, n: Int) -> Int ! { spawn } {\n    if n <= 0 { 0 } else { scope { spawn w(sp, n - 1); } 0 }\n}\n\
             fn main(sp: SpawnCap) -> Int ! { spawn } { w(sp, 2) }\n",
            extra: &[],
            status: 0,
            expect: "0",
        },
        Cycle {
            what: "a cycle that exists only across two files",
            root: "use \"cycle-b.nz\";\npub fn ping(n: Int) -> Int {\n    if n == 0 { 7 } else { pong(n - 1) }\n}\n\
             fn main() -> Int { ping(4) }\n",
            extra: &[(
                "cycle-b.nz",
                "use \"cycle-a.nz\";\npub fn pong(n: Int) -> Int {\n    if n == 0 { 9 } else { ping(n - 1) }\n}\n",
            )],
            status: 0,
            expect: "7",
        },
        Cycle {
            what: "a recursion with no bottom",
            root: "fn down(n: Int) -> Int { down(n + 1) + 1 }\nfn main() -> Int { down(0) }\n",
            extra: &[],
            status: 2,
            expect: "N0408",
        },
    ];

    for c in cases {
        let case = dir.path().join("cycle-a.nz");
        std::fs::write(&case, c.root).expect("write");
        for (name, text) in c.extra.iter() {
            std::fs::write(dir.path().join(name), text).expect("write");
        }
        let reference_exe = dir.path().join("cycle");
        let reference = Command::new(NAZM)
            .args(["build", case.to_str().unwrap()])
            .arg("-o")
            .arg(&reference_exe)
            .output()
            .expect("run nazm build");
        assert!(
            reference.status.success(),
            "{}: the reference refused it: {}",
            c.what,
            String::from_utf8_lossy(&reference.stderr)
        );
        let expected = run_native(&reference_exe);

        let (rc, ir, err) = run_stage(&emitter, &case);
        assert_eq!(
            rc,
            Some(0),
            "{}: the Nazm-written compiler refused it: {err}",
            c.what
        );
        let ll = dir.path().join("cycle.ll");
        std::fs::write(&ll, &ir).expect("write IR");
        let exe = dir.path().join("cycle-self");
        let built = Command::new("clang")
            .args(["-Wno-override-module", "-o"])
            .arg(&exe)
            .arg(&ll)
            .arg("-lpthread")
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "{}: clang rejected the IR: {}",
            c.what,
            String::from_utf8_lossy(&built.stderr)
        );
        let got = run_native(&exe);
        assert_eq!(got, expected, "{}: the two executables disagree", c.what);
        assert_eq!(got.0, Some(c.status), "{}: {got:?}", c.what);
        if c.status == 0 {
            assert_eq!(got.1.trim(), c.expect, "{}", c.what);
        } else {
            assert_eq!(got.2.as_deref(), Some(c.expect), "{}", c.what);
        }
        for (name, _) in c.extra.iter() {
            let _ = std::fs::remove_file(dir.path().join(name));
        }
    }
}

// --- N2 module and visibility parity ---------------------------------------------------
//
// # Why these exist, and what they are not
//
// `C2 ≡ C3` says the compiler written in Nazm reproduces itself. It says **nothing** about
// whether it agrees with the reference compiler on what the language is: a compiler that
// consistently accepts a program the specification forbids reaches a fixpoint too.
//
// N2 settled module ownership, visibility, non-transitive imports, collision rules, the
// root entry point and legal module cycles in the Rust front end (`docs/spec.md`). These
// run every one of those rules through **both** implementations and require the same
// accept/refuse answer — and, where the program runs, the same value. A divergence here is
// two languages sharing a name.

/// What both compilers must do with one program.
enum Verdict {
    /// Both accept, and the compiled program prints this.
    Runs(&'static str),
    /// Both refuse. The reference must give this code; the self-hosted compiler must
    /// refuse, and must agree on the code when it emits one.
    Refused(&'static str),
}

/// One module/visibility case.
struct Parity {
    what: &'static str,
    /// The program. **The first file is the root.**
    files: &'static [(&'static str, &'static str)],
    expect: Verdict,
}

/// The code in `nazm: error[NXXXX]: ...`, which is how the Nazm-written compiler reports.
fn selfhost_code(stderr: &str) -> Option<String> {
    let at = stderr.find("error[")? + "error[".len();
    let rest = stderr.get(at..)?;
    let end = rest.find(']')?;
    Some(rest.get(..end)?.to_owned())
}

/// The code in a rendered reference diagnostic, `error[NXXXX]: ...`.
fn reference_code(stderr: &str) -> Option<String> {
    selfhost_code(stderr)
}

#[test]
fn the_two_compilers_agree_on_every_module_and_visibility_rule() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());

    let cases: &[Parity] = &[
        // --- accepted --------------------------------------------------------------
        Parity {
            what: "a module calls its own private function",
            files: &[(
                "root.nz",
                "fn helper() -> Int { 7 }\nfn main() -> Int { helper() }\n",
            )],
            expect: Verdict::Runs("7"),
        },
        Parity {
            what: "a module calls an imported `pub` function",
            files: &[
                ("root.nz", "use \"lib.nz\";\nfn main() -> Int { twice(3) }\n"),
                ("lib.nz", "pub fn twice(n: Int) -> Int { n + n }\n"),
            ],
            expect: Verdict::Runs("6"),
        },
        Parity {
            what: "two modules each own a private function of the same name",
            files: &[
                (
                    "root.nz",
                    "use \"lib.nz\";\nfn size() -> Int { 1 }\nfn main() -> Int { size() + report() }\n",
                ),
                (
                    "lib.nz",
                    "pub fn report() -> Int { size() }\nfn size() -> Int { 40 }\n",
                ),
            ],
            expect: Verdict::Runs("41"),
        },
        Parity {
            what: "a forward call inside one module",
            files: &[(
                "root.nz",
                "fn main() -> Int { later() }\nfn later() -> Int { 9 }\n",
            )],
            expect: Verdict::Runs("9"),
        },
        Parity {
            what: "a diamond import is one module",
            files: &[
                (
                    "root.nz",
                    "use \"left.nz\";\nuse \"right.nz\";\nfn main() -> Int { l() + r() }\n",
                ),
                ("left.nz", "use \"shared.nz\";\npub fn l() -> Int { s() }\n"),
                ("right.nz", "use \"shared.nz\";\npub fn r() -> Int { s() }\n"),
                ("shared.nz", "pub fn s() -> Int { 21 }\n"),
            ],
            expect: Verdict::Runs("42"),
        },
        Parity {
            what: "a module import cycle, with no call cycle in it",
            files: &[
                (
                    "root.nz",
                    "use \"b.nz\";\npub fn one() -> Int { 1 }\npub fn three() -> Int { 3 }\n\
                     fn main() -> Int { two() + three() }\n",
                ),
                (
                    "b.nz",
                    "use \"root.nz\";\npub fn two() -> Int { one() + one() }\n",
                ),
            ],
            expect: Verdict::Runs("5"),
        },
        Parity {
            what: "the root's `main` is private, which is allowed",
            files: &[
                ("root.nz", "use \"lib.nz\";\nfn main() -> Int { v() }\n"),
                ("lib.nz", "pub fn v() -> Int { 4 }\n"),
            ],
            expect: Verdict::Runs("4"),
        },
        // --- refused ---------------------------------------------------------------
        Parity {
            what: "calling an imported private function",
            files: &[
                ("root.nz", "use \"lib.nz\";\nfn main() -> Int { hidden() }\n"),
                ("lib.nz", "fn hidden() -> Int { 1 }\n"),
            ],
            expect: Verdict::Refused("N0206"),
        },
        Parity {
            what: "reaching a name through a dependency's import",
            files: &[
                ("root.nz", "use \"b.nz\";\nfn main() -> Int { deep() }\n"),
                ("b.nz", "use \"c.nz\";\npub fn mid() -> Int { deep() }\n"),
                ("c.nz", "pub fn deep() -> Int { 7 }\n"),
            ],
            expect: Verdict::Refused("N0201"),
        },
        Parity {
            what: "two direct imports exporting one name",
            files: &[
                (
                    "root.nz",
                    "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { same() }\n",
                ),
                ("a.nz", "pub fn same() -> Int { 1 }\n"),
                ("b.nz", "pub fn same() -> Int { 2 }\n"),
            ],
            expect: Verdict::Refused("N0207"),
        },
        Parity {
            what: "an ambiguity nothing uses is still an ambiguity",
            files: &[
                (
                    "root.nz",
                    "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { 0 }\n",
                ),
                ("a.nz", "pub fn same() -> Int { 1 }\n"),
                ("b.nz", "pub fn same() -> Int { 2 }\n"),
            ],
            expect: Verdict::Refused("N0207"),
        },
        Parity {
            what: "an import colliding with a local definition",
            files: &[
                (
                    "root.nz",
                    "use \"lib.nz\";\nfn size() -> Int { 1 }\nfn main() -> Int { size() }\n",
                ),
                ("lib.nz", "pub fn size() -> Int { 2 }\n"),
            ],
            expect: Verdict::Refused("N0208"),
        },
        Parity {
            what: "redefining a built-in",
            files: &[(
                "root.nz",
                "fn str_len(s: Str) -> Int { 0 }\nfn main() -> Int { 1 }\n",
            )],
            expect: Verdict::Refused("N0203"),
        },
        Parity {
            what: "an imported `main` is not the program's entry point",
            files: &[
                ("root.nz", "use \"tool.nz\";\npub fn v() -> Int { 1 }\n"),
                ("tool.nz", "fn main() -> Int { 0 }\n"),
            ],
            expect: Verdict::Refused("N0204"),
        },
        Parity {
            what: "a name defined twice in one module",
            files: &[(
                "root.nz",
                "fn dup() -> Int { 1 }\nfn dup() -> Int { 2 }\nfn main() -> Int { dup() }\n",
            )],
            expect: Verdict::Refused("N0203"),
        },
    ];

    let mut divergences: Vec<String> = Vec::new();

    for (n, case) in cases.iter().enumerate() {
        let work = dir.path().join(format!("case{n}"));
        std::fs::create_dir_all(&work).expect("case dir");
        for (name, text) in case.files {
            let path = work.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parent");
            }
            std::fs::write(&path, text).expect("write");
        }
        let root = work.join(case.files.first().expect("a case has files").0);

        // The reference first, so a change to it is reported as a change to it rather
        // than as a self-hosted defect.
        let reference = Command::new(NAZM)
            .args(["check", root.to_str().unwrap()])
            .output()
            .expect("run the reference");
        let ref_err = String::from_utf8_lossy(&reference.stderr).into_owned();
        let ref_ok = reference.status.success();

        let (rc, ir, ir_err) = run_stage(&emitter, &root);
        let self_ok = rc == Some(0);

        match case.expect {
            Verdict::Runs(value) => {
                if !ref_ok {
                    divergences.push(format!(
                        "{}: the reference refused a program it should accept:\n{ref_err}",
                        case.what
                    ));
                    continue;
                }
                if !self_ok {
                    divergences.push(format!(
                        "{}: the self-hosted compiler refused a program the reference accepts:\n{ir_err}",
                        case.what
                    ));
                    continue;
                }
                // Both accepted. They must also agree on what it means.
                let ll = work.join("out.ll");
                std::fs::write(&ll, &ir).expect("write IR");
                let exe = work.join("out");
                let built = Command::new("clang")
                    .args(["-Wno-override-module", "-o"])
                    .arg(&exe)
                    .arg(&ll)
                    .output()
                    .expect("run clang");
                if !built.status.success() {
                    divergences.push(format!(
                        "{}: clang rejected the self-hosted IR:\n{}",
                        case.what,
                        String::from_utf8_lossy(&built.stderr)
                    ));
                    continue;
                }
                let (code, out, _) = run_bounded(
                    &mut common::bounded_exe(&exe),
                    std::time::Duration::from_secs(60),
                )
                .expect("the compiled program hung");
                if code != Some(0) || out.trim() != value {
                    divergences.push(format!(
                        "{}: the self-hosted build printed {:?}, expected {value:?}",
                        case.what,
                        out.trim()
                    ));
                    continue;
                }
                let interpreted = Command::new(NAZM)
                    .args(["run", "--interpret", root.to_str().unwrap()])
                    .output()
                    .expect("run");
                let got = String::from_utf8_lossy(&interpreted.stdout)
                    .trim()
                    .to_owned();
                if got != value {
                    divergences.push(format!(
                        "{}: the reference ran it to {got:?}, expected {value:?}",
                        case.what
                    ));
                }
            }
            Verdict::Refused(code) => {
                if ref_ok {
                    divergences.push(format!(
                        "{}: the reference accepted a program it should refuse",
                        case.what
                    ));
                    continue;
                }
                match reference_code(&ref_err) {
                    Some(c) if c == code => {}
                    other => divergences.push(format!(
                        "{}: the reference gave {other:?}, expected {code:?}",
                        case.what
                    )),
                }
                if self_ok {
                    divergences.push(format!(
                        "{}: the self-hosted compiler ACCEPTED a program the reference \
                         refuses with {code}",
                        case.what
                    ));
                    continue;
                }
                // It refused. When it names a code, that code must be the same one:
                // an agreement on "no" that disagrees on "why" is half a parity.
                if let Some(c) = selfhost_code(&ir_err) {
                    if c != code {
                        divergences.push(format!(
                            "{}: the self-hosted compiler refused with {c}, the reference \
                             with {code}",
                            case.what
                        ));
                    }
                }
            }
        }
    }

    assert!(
        divergences.is_empty(),
        "{} of {} module/visibility rules diverge between the two compilers:\n\n{}",
        divergences.len(),
        cases.len(),
        divergences.join("\n\n")
    );
}

// ---------------------------------------------------------------------------
// N98: the parity record — what the compiler written in Nazm does with each feature the
// reference compiler builds, measured, and kept as `compiler/parity.json`.
// ---------------------------------------------------------------------------

/// Run a native executable, bounded: its exit status, standard output, and the diagnostic code a
/// failure names, if any.
fn run_native(exe: &Path) -> (Option<i32>, String, Option<String>) {
    let (rc, out, err) = run_bounded(&mut common::bounded_exe(exe), STAGE_LIMIT)
        .unwrap_or_else(|| panic!("{} did not finish", exe.display()));
    let code = err
        .split("error[")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .map(str::to_owned);
    (rc, out, code)
}

/// Every probe in `compiler/parity/`, built by both compilers and run: `equal` when the program
/// built by the compiler written in Nazm exits and prints as the reference's does, `refused` when
/// that compiler does not build it — a gap, recorded with the code it gives — and `differs` when it
/// builds something that behaves otherwise, which is a defect.
#[test]
fn the_parity_record_is_what_both_compilers_do_with_each_feature() {
    let dir = tempfile::tempdir().expect("tempdir");
    let emitter = build_emitter(dir.path());
    let probes_dir = repo_root().join("compiler/parity");
    let mut probes: Vec<PathBuf> = std::fs::read_dir(&probes_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "nz") && p.file_stem().is_some_and(|s| s != "helper")
        })
        .collect();
    probes.sort();
    let mut rows = Vec::new();
    let mut differs = Vec::new();
    for probe in &probes {
        let name = probe.file_stem().unwrap().to_string_lossy().into_owned();
        let reference = dir.path().join(format!("{name}.ref"));
        let built = Command::new(NAZM)
            .current_dir(&probes_dir)
            .args(["build", "--no-cache"])
            .arg(probe)
            .arg("-o")
            .arg(&reference)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "the reference refuses its own probe {name}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let expected = run_native(&reference);
        let (rc, ir, err) = run_stage(&emitter, probe);
        let (verdict, detail) = if rc == Some(0) {
            let ll = dir.path().join(format!("{name}.ll"));
            std::fs::write(&ll, &ir).unwrap();
            let exe = dir.path().join(format!("{name}.self"));
            let linked = Command::new("clang")
                .args(["-Wno-override-module", "-o"])
                .arg(&exe)
                .arg(&ll)
                .arg("-lpthread")
                .output()
                .unwrap();
            if linked.status.success() {
                let got = run_native(&exe);
                if got == expected {
                    ("equal", String::new())
                } else {
                    differs.push(format!("{name}: reference {expected:?}, selfhost {got:?}"));
                    ("differs", String::new())
                }
            } else {
                // IR the emitter wrote that does not link is a defect, not a refusal.
                differs.push(format!(
                    "{name}: emitted IR does not link: {}",
                    String::from_utf8_lossy(&linked.stderr)
                ));
                ("differs", String::new())
            }
        } else {
            let code = err
                .split("error[")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .unwrap_or("unnamed")
                .to_owned();
            ("refused", code)
        };
        let dimension = name.split('-').next().unwrap_or("").to_owned();
        rows.push(serde_json::json!({
            "probe": name,
            "dimension": dimension,
            "selfhost": verdict,
            "refusal": if detail.is_empty() { serde_json::Value::Null } else { serde_json::json!(detail) },
        }));
    }
    let equal = rows.iter().filter(|r| r["selfhost"] == "equal").count();
    let record = serde_json::json!({
        "schema": "nazm.parity/1",
        "reference": "nazm build, run natively",
        "selfhost": "compiler/emit.nz, built by the reference, its LLVM IR linked by clang",
        "probes": rows.len(),
        "equal": equal,
        "refused": rows.len() - equal - differs.len(),
        "differs": differs.len(),
        "results": rows,
    });
    let text = format!("{record:#}\n");
    // A disagreement is a defect whatever the record says.
    assert!(differs.is_empty(), "{}", differs.join("\n"));
    let committed =
        std::fs::read_to_string(repo_root().join("compiler/parity.json")).unwrap_or_default();
    assert_eq!(
        committed, text,
        "compiler/parity.json is not what the compilers do; the measured record:\n{text}"
    );
}
