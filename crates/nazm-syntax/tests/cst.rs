//! The lossless concrete syntax tree, N15: what it keeps, and what recovery keeps in it.
//!
//! Two properties are the milestone, and each has a test that cannot pass without it:
//!
//! - **Losslessness.** The leaves of the tree, concatenated, are the input — every byte,
//!   in order, for valid and invalid text alike. Checked over every `.nz` file in the
//!   repository, every example in `docs/user/grammar.ebnf`, a set of malformed fixtures, and
//!   seeded random input.
//! - **Local recovery.** A statement that does not parse costs that statement, not the
//!   rest of its function: what follows is still structured syntax in the tree.
//!
//! And one that makes the first two worth having: the tree the checker reads is built by
//! the same decisions, so the two cannot disagree about where a construct is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, Span};
use nazm_syntax::{
    parse_both, parse_cst, Block, Cst, Expr, PlaceStep, Program, Stmt, SyntaxKind, MAX_NESTING,
    PARSE_STACK_BYTES,
};

/// Run `f` on the stack `parse_source` documents as its requirement.
fn on_the_documented_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

fn cst(text: &str) -> (Cst, Vec<Diagnostic>) {
    let parsed = parse_cst(FileId::ROOT, text);
    (parsed.cst, parsed.diagnostics)
}

/// Every structural invariant the tree promises, for any input.
///
/// - the leaves, concatenated, are `text`;
/// - each leaf's span is exactly where its bytes are in `text`, and the spans are
///   contiguous from 0 to the end: every byte is in exactly one leaf;
/// - no leaf is empty — nothing is synthetic, and the end of the file is not a leaf;
/// - no node other than the root begins or ends with trivia, so a node's span is its
///   construct and nothing else;
/// - every span names the file the tree was parsed as.
fn assert_lossless(tree: &Cst, text: &str, what: &str) {
    assert_eq!(
        tree.text(),
        text,
        "{what}: the tree does not reproduce its input"
    );
    let mut at = 0u32;
    for leaf in tree.leaves() {
        assert_eq!(
            leaf.span.file,
            tree.file(),
            "{what}: a leaf names another file"
        );
        assert_eq!(
            leaf.span.start, at,
            "{what}: a gap or an overlap before {leaf:?}"
        );
        assert!(
            leaf.span.end > leaf.span.start,
            "{what}: an empty leaf {leaf:?}"
        );
        let bytes = text.get(leaf.span.start as usize..leaf.span.end as usize);
        assert_eq!(
            bytes,
            Some(leaf.text),
            "{what}: {leaf:?} is not its source bytes"
        );
        assert!(
            !leaf.kind.is_node(),
            "{what}: a node kind on a leaf {leaf:?}"
        );
        at = leaf.span.end;
    }
    assert_eq!(
        at as usize,
        text.len(),
        "{what}: the leaves stop short of the end"
    );
    for node in tree.nodes() {
        assert!(node.kind().is_node(), "{what}: a leaf kind on a node");
        if node.kind() == SyntaxKind::SourceFile {
            continue;
        }
        let leaves = node.leaves();
        let (Some(first), Some(last)) = (leaves.first(), leaves.last()) else {
            panic!("{what}: an empty {:?} node", node.kind());
        };
        assert!(
            !first.kind.is_trivia() && !last.kind.is_trivia(),
            "{what}: {:?} {:?} begins or ends with trivia",
            node.kind(),
            node.text()
        );
    }
}

fn kinds(tree: &Cst) -> Vec<SyntaxKind> {
    tree.nodes().iter().map(|n| n.kind()).collect()
}

fn texts_of(tree: &Cst, kind: SyntaxKind) -> Vec<String> {
    tree.nodes()
        .iter()
        .filter(|n| n.kind() == kind)
        .map(|n| n.text())
        .collect()
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Every `.nz` file under the directories that hold Nazm source, sorted.
fn corpus() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "nz") {
                out.push(path);
            }
        }
    }
    let root = workspace();
    let mut paths = Vec::new();
    for dir in [
        "compiler",
        "library",
        "examples",
        "bench/programs",
        "review",
        "crates/nazm-cli/tests",
    ] {
        walk(&root.join(dir), &mut paths);
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let name = p.strip_prefix(&root).unwrap_or(&p).display().to_string();
            let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, text)
        })
        .collect()
}

/// Every `@example` in `docs/user/grammar.ebnf`, each a program the grammar says is valid.
fn grammar_examples() -> Vec<String> {
    let text =
        std::fs::read_to_string(workspace().join("docs/user/grammar.ebnf")).expect("grammar");
    // Written `(* @example … *)` on a line of its own, or `* @example …` inside a longer
    // comment — the two forms `tests/grammar.rs` reads.
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("(* @example ")
                .or_else(|| l.strip_prefix("* @example "))
        })
        .map(|e| e.trim_end_matches("*)").trim().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------------------
// The milestone, stated as one test.
// ---------------------------------------------------------------------------------------

#[test]
fn a_lossless_tree_preserves_every_byte_and_recovers_inside_a_function() {
    // A leading comment, tabs and odd spacing, a valid statement, a malformed one, a
    // comment, a valid statement after the error, the final expression, trailing
    // whitespace and a trailing comment with no newline after it.
    let text = "// leading comment\n\n\
                fn main()  ->\tInt {\n\
                \tlet a = 1;   \n\
                \n\
                    let broken = ;\n\
                    // a comment after the mistake\n\
                    let b =\t2;\n\
                    b\n\
                }   \n\n// trailing comment";
    let (tree, diagnostics) = on_the_documented_stack(|| cst(text));

    // Exactly the input, byte for byte.
    assert_lossless(&tree, text, "the acceptance fixture");

    // One diagnostic, for the one mistake, where it is.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].code, codes::UNEXPECTED_TOKEN);
    let at = text.find("= ;").unwrap() + 2;
    assert_eq!(diagnostics[0].primary.span.start as usize, at);

    // The malformed statement is an error node over exactly its bytes...
    assert_eq!(texts_of(&tree, SyntaxKind::Error), ["let broken = ;"]);
    // ...and what follows it is still structured syntax, inside the same block.
    assert_eq!(
        texts_of(&tree, SyntaxKind::LetStmt),
        ["let a = 1;", "let b =\t2;"]
    );
    let block = tree
        .nodes()
        .into_iter()
        .find(|n| n.kind() == SyntaxKind::Block)
        .expect("the body is a block");
    let children: Vec<SyntaxKind> = block.children().iter().map(|c| c.kind()).collect();
    assert_eq!(
        children,
        [
            SyntaxKind::LetStmt,
            SyntaxKind::Error,
            SyntaxKind::LetStmt,
            SyntaxKind::NameRef
        ],
        "the statement after the error and the tail are the block's, in order"
    );
    // The comment after the mistake is in the tree, in order, not in a side list.
    let comments: Vec<&str> = tree
        .leaves()
        .into_iter()
        .filter(|l| l.kind == SyntaxKind::Comment)
        .map(|l| l.text)
        .collect();
    assert_eq!(
        comments,
        [
            "// leading comment",
            "// a comment after the mistake",
            "// trailing comment"
        ]
    );
}

// ---------------------------------------------------------------------------------------
// Losslessness.
// ---------------------------------------------------------------------------------------

#[test]
fn every_nazm_file_in_the_repository_round_trips_exactly() {
    let files = corpus();
    // The count is a floor, not a pin: a corpus that silently shrank would prove less.
    assert!(files.len() >= 190, "only {} .nz files found", files.len());
    on_the_documented_stack(move || {
        let mut clean = 0;
        for (name, text) in &files {
            let (tree, diagnostics) = cst(text);
            assert_lossless(&tree, text, name);
            // A tree has an error region exactly when a diagnostic says why.
            let malformed = kinds(&tree).contains(&SyntaxKind::Error)
                || tree.leaves().iter().any(|l| l.kind == SyntaxKind::Invalid);
            assert_eq!(
                malformed,
                !diagnostics.is_empty(),
                "{name}: error regions and diagnostics disagree: {diagnostics:#?}"
            );
            if diagnostics.is_empty() {
                clean += 1;
            }
        }
        // The compiler, the prelude and the conformance programs are all clean; the few
        // files that are not are the syntax refusals, which are malformed on purpose.
        assert!(
            clean >= 180,
            "only {clean} files parsed without a diagnostic"
        );
    });
}

#[test]
fn the_compiler_written_in_nazm_and_the_prelude_parse_clean_and_round_trip() {
    // Named separately: these are the largest valid inputs there are, and the ones the
    // milestone's parity claim is about.
    let root = workspace();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("compiler"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "nz"))
        .collect();
    paths.push(root.join("library/core/prelude.nz"));
    assert!(paths.len() >= 9, "{paths:?}");
    on_the_documented_stack(move || {
        for p in paths {
            let text = std::fs::read_to_string(&p).unwrap();
            let (tree, diagnostics) = cst(&text);
            assert!(diagnostics.is_empty(), "{}: {diagnostics:#?}", p.display());
            assert_lossless(&tree, &text, &p.display().to_string());
            assert!(
                !kinds(&tree).contains(&SyntaxKind::Error),
                "{}",
                p.display()
            );
        }
    });
}

#[test]
fn every_grammar_example_parses_clean_and_round_trips() {
    let examples = grammar_examples();
    assert!(examples.len() > 20, "{} examples", examples.len());
    for example in examples {
        let (tree, diagnostics) = cst(&example);
        assert!(diagnostics.is_empty(), "{example}: {diagnostics:#?}");
        assert_lossless(&tree, &example, &example);
    }
}

#[test]
fn the_ends_of_a_file_are_kept_and_the_end_itself_owns_nothing() {
    for (text, what) in [
        ("", "empty"),
        ("   \t\n\n", "only whitespace"),
        ("// only a comment", "only a comment, no newline"),
        ("// only a comment\n", "only a comment"),
        ("fn main() -> Int { 0 }", "no trailing newline"),
        ("fn main() -> Int { 0 }\n", "one trailing newline"),
        ("fn main() -> Int { 0 }\n\n\n", "several trailing newlines"),
        ("\n\n  fn main() -> Int { 0 }", "leading blank lines"),
        ("fn main() -> Int { 0 }\r\n", "a CRLF line ending"),
    ] {
        let (tree, diagnostics) = cst(text);
        assert_lossless(&tree, text, what);
        assert_eq!(tree.root().span().end as usize, text.len(), "{what}");
        assert!(diagnostics.is_empty(), "{what}: {diagnostics:#?}");
    }
    // An empty file is a root with nothing in it: no leaf stands for the end.
    let (tree, _) = cst("");
    assert!(tree.leaves().is_empty());
    assert_eq!(kinds(&tree), [SyntaxKind::SourceFile]);
}

#[test]
fn every_token_keeps_the_bytes_it_was_written_with() {
    // Spellings a canonical form would change: separators in an integer, the text of a
    // string exactly, a name that is almost a keyword, spacing inside a construct.
    let text = "fn main() -> Int {\n    let fnord = 1_000_000;\n    let s = \"a  b\";\n    fnord  +   str_len(s)\n}\n";
    let (tree, diagnostics) = cst(text);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_lossless(&tree, text, "spellings");
    let leaves = tree.leaves();
    let spelled = |kind: SyntaxKind| -> Vec<&str> {
        leaves
            .iter()
            .filter(|l| l.kind == kind)
            .map(|l| l.text)
            .collect()
    };
    assert_eq!(spelled(SyntaxKind::Int), ["1_000_000"]);
    assert_eq!(spelled(SyntaxKind::Str), ["\"a  b\""]);
    assert!(spelled(SyntaxKind::Ident).contains(&"fnord"));
    assert_eq!(
        texts_of(&tree, SyntaxKind::BinaryExpr),
        ["fnord  +   str_len(s)"]
    );
}

#[test]
fn punctuation_is_in_the_tree_where_the_abstract_tree_has_none() {
    let text = "fn f[T: Equality](a: T, b: T) -> Bool { (a == b) }\n\
                fn main() -> Int { let r = Box[Int](value: 1); r.value }";
    let (tree, diagnostics) = cst(text);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_lossless(&tree, text, "punctuation");
    let kinds: Vec<SyntaxKind> = tree.leaves().iter().map(|l| l.kind).collect();
    for k in [
        SyntaxKind::LBracket,
        SyntaxKind::Colon,
        SyntaxKind::RBracket,
        SyntaxKind::Comma,
        SyntaxKind::Arrow,
        SyntaxKind::LParen,
        SyntaxKind::RParen,
        SyntaxKind::Semi,
        SyntaxKind::Dot,
        SyntaxKind::EqEq,
    ] {
        assert!(kinds.contains(&k), "{k:?} missing from the leaves");
    }
    // The parentheses around `a == b` are a node, although the abstract tree has none.
    assert_eq!(texts_of(&tree, SyntaxKind::ParenExpr), ["(a == b)"]);
}

#[test]
fn text_the_lexer_refuses_is_kept_as_the_bytes_it_was() {
    // Unknown characters, a multi-byte one among them, and a string with no closing
    // quote. Each is a diagnostic *and* an invalid leaf holding exactly its bytes.
    let text =
        "fn main() -> Int {\n    let a = 1 $ 2;\n    let é = 3;\n    let s = \"open\n    a\n}\n";
    let (tree, diagnostics) = cst(text);
    assert_lossless(&tree, text, "lexical errors");
    let invalid: Vec<&str> = tree
        .leaves()
        .into_iter()
        .filter(|l| l.kind == SyntaxKind::Invalid)
        .map(|l| l.text)
        .collect();
    assert_eq!(invalid, ["$", "é", "\"open"]);
    assert!(diagnostics
        .iter()
        .any(|d| d.code == codes::UNKNOWN_CHARACTER));
    assert!(diagnostics
        .iter()
        .any(|d| d.code == codes::UNTERMINATED_STRING));
}

#[test]
fn non_ascii_text_is_kept_where_the_lexer_accepts_it_and_nowhere_new() {
    // Inside a string literal and a comment it is ordinary text. As a name it is still
    // an unknown character — N15 changes what is kept, not what is valid.
    let valid = "// café ☕\nfn main() -> Int { str_len(\"naïve ✓\") }\n";
    let (tree, diagnostics) = cst(valid);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_lossless(&tree, valid, "non-ASCII text");
    // Byte offsets, not characters: `✓` is three bytes and the span says so.
    let s = tree
        .leaves()
        .into_iter()
        .find(|l| l.kind == SyntaxKind::Str)
        .unwrap();
    assert_eq!((s.span.end - s.span.start) as usize, "\"naïve ✓\"".len());

    let (_, diagnostics) = cst("fn café() -> Int { 1 }");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == codes::UNKNOWN_CHARACTER),
        "a non-ASCII name must stay refused: {diagnostics:#?}"
    );
}

#[test]
fn a_tree_is_of_one_file_and_says_which() {
    let file = FileId::from_index(3);
    let text = "fn helper() -> Int { 1 }\n";
    let parsed = parse_cst(file, text);
    assert_eq!(parsed.cst.file(), file);
    assert!(parsed.cst.leaves().iter().all(|l| l.span.file == file));
    assert!(parsed.cst.nodes().iter().all(|n| n.span().file == file));
}

// ---------------------------------------------------------------------------------------
// One lexical truth, one parser.
// ---------------------------------------------------------------------------------------

#[test]
fn the_formatters_comments_and_the_trees_comments_are_the_same_list() {
    // The formatter reads comments through `lex_with_comments`, which is derived from the
    // same lossless scan the tree is built from. `// inside a string` is not a comment
    // to either.
    let text = "// one\nfn main() -> Int { // two\n    str_len(\"// not a comment\") // three\n}\n";
    let (_, _, comments) = nazm_syntax::lex_with_comments(text);
    let (tree, _) = cst(text);
    let from_tree: Vec<Span> = tree
        .leaves()
        .into_iter()
        .filter(|l| l.kind == SyntaxKind::Comment)
        .map(|l| l.span)
        .collect();
    assert_eq!(comments, from_tree);
    assert_eq!(from_tree.len(), 3);
}

#[test]
fn the_parsers_tokens_are_exactly_the_trees_non_trivia_leaves() {
    // The parser's token list and the tree's token leaves come from one scan, so they are
    // the same tokens with the same spans — the tree adds trivia and nothing else.
    on_the_documented_stack(|| {
        for (name, text) in corpus() {
            let (tokens, _) = nazm_syntax::lex(&text);
            let (tree, _) = cst(&text);
            let leaves: Vec<Span> = tree
                .leaves()
                .into_iter()
                .filter(|l| l.kind.is_token())
                .map(|l| l.span)
                .collect();
            let spans: Vec<Span> = tokens
                .iter()
                .filter(|t| t.kind != nazm_syntax::TokenKind::Eof)
                .map(|t| t.span)
                .collect();
            assert_eq!(leaves, spans, "{name}");
        }
    });
}

/// Every construct the abstract tree records, with the concrete kind that must have the
/// same span. Operators are checked by their token instead: an abstract binary node's span
/// starts at its operand's, which excludes an opening parenthesis the concrete node owns.
fn abstract_constructs(program: &Program) -> (Vec<(SyntaxKind, Span)>, Vec<Span>) {
    enum Node<'a> {
        Expr(&'a Expr),
        Block(&'a Block),
    }
    let mut spans = Vec::new();
    let mut operators = Vec::new();
    let mut stack: Vec<Node<'_>> = Vec::new();
    for i in &program.imports {
        spans.push((SyntaxKind::UseItem, i.span));
    }
    for r in &program.records {
        spans.push((SyntaxKind::RecordItem, r.span));
        for f in &r.fields {
            spans.push((SyntaxKind::Field, f.span));
        }
    }
    for e in &program.enums {
        spans.push((SyntaxKind::EnumItem, e.span));
        for v in &e.variants {
            spans.push((SyntaxKind::Variant, v.span));
            for f in &v.fields {
                spans.push((SyntaxKind::Field, f.span));
            }
        }
    }
    // A trait's methods (N78): their parameters are constructs; the trait and its methods are
    // items the checker reads, compared by their own kinds below.
    for t in &program.traits {
        spans.push((SyntaxKind::TraitItem, t.span));
        for m in &t.methods {
            spans.push((SyntaxKind::TraitMethod, m.span));
            for p in &m.params {
                spans.push((SyntaxKind::Param, p.span));
            }
        }
    }
    // A constant's or a static's value is an expression like any other (Gate 2, Gate 3): a static
    // atomic's `atomic_new(0)` is a call both trees must see.
    for c in &program.consts {
        stack.push(Node::Expr(&c.value));
    }
    for f in &program.functions {
        // A foreign declaration (N85) is a function item with parameters and no body.
        spans.push((SyntaxKind::FnItem, f.span));
        // The `out: OutCap` a `main` is given (R1, 5b.1) has no width: nothing wrote it, so
        // the concrete tree has no node for it. (The `Int` and the `0` a function with no
        // result type is given, 5b.2, are no construct here either.)
        for p in f.params.iter().filter(|p| p.span.start != p.span.end) {
            spans.push((SyntaxKind::Param, p.span));
        }
        // A contract clause and its expression are constructs (N87).
        for c in &f.contracts {
            spans.push((SyntaxKind::Contract, c.span));
            stack.push(Node::Expr(&c.expr));
        }
        if let Some(body) = &f.body {
            stack.push(Node::Block(body));
        }
    }
    while let Some(node) = stack.pop() {
        match node {
            Node::Block(b) => {
                spans.push((SyntaxKind::Block, b.span));
                for s in &b.statements {
                    match s {
                        Stmt::Let(l) => {
                            spans.push((SyntaxKind::LetStmt, l.span));
                            stack.push(Node::Expr(&l.value));
                        }
                        Stmt::Assign(a) => {
                            spans.push((SyntaxKind::AssignStmt, a.span));
                            spans.push((SyntaxKind::Place, a.place.span));
                            for step in &a.place.path {
                                if let PlaceStep::Index { index, .. } = step {
                                    stack.push(Node::Expr(index));
                                }
                            }
                            // `x += e` is `x = x + e` (R1, 5b.12): the read of `x` is the place
                            // again, which the concrete tree has as the place and not as a name.
                            match &a.value {
                                Expr::Binary { lhs, rhs, .. } if lhs.span() == a.place.span => {
                                    stack.push(Node::Expr(rhs));
                                }
                                v => stack.push(Node::Expr(v)),
                            }
                        }
                        Stmt::Return(r) => {
                            spans.push((SyntaxKind::ReturnStmt, r.span));
                            stack.push(Node::Expr(&r.value));
                        }
                        Stmt::While(w) => {
                            spans.push((SyntaxKind::WhileStmt, w.span));
                            stack.push(Node::Expr(&w.cond));
                            stack.push(Node::Block(&w.body));
                        }
                        Stmt::For(f) => {
                            spans.push((SyntaxKind::ForStmt, f.span));
                            for e in f.iter.exprs() {
                                stack.push(Node::Expr(e));
                            }
                            stack.push(Node::Block(&f.body));
                        }
                        Stmt::Expr(e) => stack.push(Node::Expr(e)),
                        Stmt::Scope(sc) => {
                            spans.push((SyntaxKind::ScopeStmt, sc.span));
                            stack.push(Node::Block(&sc.body));
                        }
                        Stmt::Defer(df) => {
                            spans.push((SyntaxKind::DeferStmt, df.span));
                            stack.push(Node::Block(&df.body));
                        }
                        Stmt::Spawn(sp) => {
                            spans.push((SyntaxKind::SpawnStmt, sp.span));
                            for a in &sp.args {
                                stack.push(Node::Expr(a));
                            }
                        }
                        Stmt::Break { span } => spans.push((SyntaxKind::BreakStmt, *span)),
                        Stmt::Continue { span } => {
                            spans.push((SyntaxKind::ContinueStmt, *span));
                        }
                    }
                }
                if let Some(t) = &b.tail {
                    stack.push(Node::Expr(t));
                }
            }
            Node::Expr(e) => match e {
                Expr::Int { .. }
                | Expr::WideInt { .. }
                | Expr::Float { .. }
                | Expr::Bool { .. }
                | Expr::Str { .. } => {}
                // One token in the concrete tree (L4): its holes are parsed apart and have no
                // nodes there. What `print(x)` and `println(x)` are read as (R1, 5b.13) is no
                // token at all: an interpolation over exactly its value, whose nodes are the
                // value's.
                Expr::Interp { parts, span } => {
                    for p in parts {
                        if let nazm_syntax::InterpPart::Hole { expr, .. } = p {
                            if expr.span() == *span {
                                stack.push(Node::Expr(expr));
                            }
                        }
                    }
                }
                Expr::Var { span, .. } => spans.push((SyntaxKind::NameRef, *span)),
                Expr::Neg {
                    operand, op_span, ..
                }
                | Expr::Not {
                    operand, op_span, ..
                }
                | Expr::BitNot {
                    operand, op_span, ..
                } => {
                    operators.push(*op_span);
                    stack.push(Node::Expr(operand));
                }
                Expr::Binary {
                    lhs, rhs, op_span, ..
                } => {
                    operators.push(*op_span);
                    stack.push(Node::Expr(lhs));
                    stack.push(Node::Expr(rhs));
                }
                Expr::Call { args, span, .. } => {
                    spans.push((SyntaxKind::CallExpr, *span));
                    for a in args {
                        stack.push(Node::Expr(a));
                    }
                }
                Expr::Construct { fields, span, .. } => {
                    spans.push((SyntaxKind::ConstructExpr, *span));
                    for f in fields {
                        spans.push((SyntaxKind::FieldInit, f.span));
                        stack.push(Node::Expr(&f.value));
                    }
                }
                // `Some(x)` and `None` (R1, 5b.14) are written as a call and a name, and
                // `Color.Red` as a field: the concrete tree holds what was written.
                Expr::Variant {
                    fields,
                    span,
                    ty_span,
                    variant_span,
                    ..
                } if ty_span == variant_span => {
                    if fields.is_empty() {
                        spans.push((SyntaxKind::NameRef, *span));
                    } else {
                        spans.push((SyntaxKind::CallExpr, *span));
                        for f in fields {
                            stack.push(Node::Expr(&f.value));
                        }
                    }
                }
                Expr::Variant {
                    fields,
                    span,
                    ty_span,
                    variant_span,
                    ..
                } if fields.is_empty() && span.end == variant_span.end => {
                    spans.push((SyntaxKind::FieldExpr, *span));
                    spans.push((SyntaxKind::NameRef, *ty_span));
                }
                Expr::Variant { fields, span, .. } => {
                    spans.push((SyntaxKind::VariantExpr, *span));
                    for f in fields {
                        spans.push((SyntaxKind::FieldInit, f.span));
                        stack.push(Node::Expr(&f.value));
                    }
                }
                Expr::Match {
                    scrutinee,
                    arms,
                    source,
                    span,
                } => {
                    // What `if let` and `let … else` mean is a `match` with no `match` written
                    // (R1a): only a written one's arms are nodes of the concrete tree.
                    match source {
                        nazm_syntax::MatchSource::Written => {
                            spans.push((SyntaxKind::MatchExpr, *span));
                            for a in arms {
                                spans.push((SyntaxKind::MatchArm, a.span));
                            }
                        }
                        nazm_syntax::MatchSource::IfLet => {
                            spans.push((SyntaxKind::IfLetExpr, *span));
                        }
                        nazm_syntax::MatchSource::LetElse | nazm_syntax::MatchSource::Let => {}
                    }
                    stack.push(Node::Expr(scrutinee));
                    for a in arms {
                        // An `if let` with no `else` is given an empty one, which nothing wrote:
                        // a block of no width the concrete tree has no node for.
                        let synthetic = *source == nazm_syntax::MatchSource::IfLet
                            && matches!(&a.body, Expr::Block(b) if b.span.start == b.span.end);
                        if !synthetic {
                            stack.push(Node::Expr(&a.body));
                        }
                    }
                }
                Expr::Field { base, .. } => stack.push(Node::Expr(base)),
                Expr::Array { elems, span } => {
                    spans.push((SyntaxKind::ArrayExpr, *span));
                    for x in elems {
                        stack.push(Node::Expr(x));
                    }
                }
                Expr::Tuple { elems, span } => {
                    spans.push((SyntaxKind::TupleExpr, *span));
                    for x in elems {
                        stack.push(Node::Expr(x));
                    }
                }
                Expr::ArrayRepeat { value, count, span } => {
                    spans.push((SyntaxKind::ArrayExpr, *span));
                    stack.push(Node::Expr(value));
                    stack.push(Node::Expr(count));
                }
                Expr::Index {
                    base, index, span, ..
                } => {
                    spans.push((SyntaxKind::IndexExpr, *span));
                    stack.push(Node::Expr(base));
                    stack.push(Node::Expr(index));
                }
                Expr::Propagate {
                    operand, op_span, ..
                } => {
                    operators.push(*op_span);
                    stack.push(Node::Expr(operand));
                }
                Expr::If {
                    cond,
                    then_block,
                    else_block,
                    span,
                } => {
                    spans.push((SyntaxKind::IfExpr, *span));
                    stack.push(Node::Expr(cond));
                    stack.push(Node::Block(then_block));
                    if let Some(b) = else_block {
                        stack.push(Node::Block(b));
                    }
                }
                Expr::Block(b) => stack.push(Node::Block(b)),
                Expr::Closure {
                    params, body, span, ..
                } => {
                    spans.push((SyntaxKind::ClosureExpr, *span));
                    // A closure's parameters are constructs as a function's are.
                    for p in params {
                        spans.push((SyntaxKind::Param, p.span));
                    }
                    stack.push(Node::Block(body));
                }
                Expr::MethodCall {
                    receiver,
                    args,
                    span,
                    ..
                } => {
                    spans.push((SyntaxKind::MethodCallExpr, *span));
                    stack.push(Node::Expr(receiver));
                    for a in args {
                        stack.push(Node::Expr(a));
                    }
                }
            },
        }
    }
    spans.sort_by_key(|(k, s)| (*k, s.start, s.end));
    operators.sort_by_key(|s| (s.start, s.end));
    (spans, operators)
}

#[test]
fn the_abstract_and_concrete_trees_agree_on_every_construct() {
    // The claim that there is one parser, tested from outside it: for every valid file,
    // each construct the checker will see has a concrete node of the matching kind over
    // exactly the same bytes, and nothing of those kinds exists that the checker will not
    // see. Two parsers that accepted different grammars could not both pass this.
    on_the_documented_stack(|| {
        let mut constructs = 0usize;
        for (name, text) in corpus() {
            let (program, tree, diagnostics) = parse_both(FileId::ROOT, &text);
            if !diagnostics.is_empty() {
                continue;
            }
            let (want, operators) = abstract_constructs(&program);
            let checked: Vec<SyntaxKind> = {
                let mut k: Vec<SyntaxKind> = want.iter().map(|(k, _)| *k).collect();
                k.dedup();
                k
            };
            let mut have: Vec<(SyntaxKind, Span)> = tree
                .nodes()
                .iter()
                .filter(|n| checked.contains(&n.kind()))
                .map(|n| (n.kind(), n.span()))
                .collect();
            have.sort_by_key(|(k, s)| (*k, s.start, s.end));
            // Every kind the abstract side recorded is compared in full; a kind a file
            // never uses is absent on both sides and not compared.
            assert_eq!(have, want, "{name}: the trees disagree about a construct");
            let mut ops: Vec<Span> = tree
                .nodes()
                .iter()
                .filter(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::BinaryExpr
                            | SyntaxKind::NegExpr
                            | SyntaxKind::NotExpr
                            | SyntaxKind::BitNotExpr
                            | SyntaxKind::PropagateExpr
                    )
                })
                .filter_map(|n| {
                    n.child_leaves()
                        .into_iter()
                        .find(|l| l.kind.is_token())
                        .map(|l| l.span)
                })
                .collect();
            ops.sort_by_key(|s| (s.start, s.end));
            // A folded `-9223372036854775808` is one abstract `Int` and a concrete `-`
            // over a literal, so the concrete side may have operators the abstract has
            // folded away; every abstract operator is a concrete one.
            let missing: Vec<&Span> = operators.iter().filter(|o| !ops.contains(o)).collect();
            assert!(
                missing.is_empty(),
                "{name}: an operator the checker sees is not an operator in the tree: {missing:?}"
            );
            constructs += want.len();
        }
        assert!(constructs > 10_000, "only {constructs} constructs compared");
    });
}

// ---------------------------------------------------------------------------------------
// Recovery.
// ---------------------------------------------------------------------------------------

/// A malformed fixture: what it is, the text, the code of the one diagnostic, the text it
/// points at, and the text of a construct after the error that must still be in the tree.
struct Fixture {
    what: &'static str,
    text: &'static str,
    code: nazm_diag::Code,
    at: &'static str,
    after: (SyntaxKind, &'static str),
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            what: "a malformed let in the middle of a function",
            text: "fn main() -> Int {\n    let a = 1;\n    let broken = ;\n    let b = 2;\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ";\n    let b = 2",
            after: (SyntaxKind::LetStmt, "let b = 2;"),
        },
        Fixture {
            what: "a missing semicolon",
            // On one line: a line break would end the `let` (R1, 5b.19).
            text: "fn main() -> Int {\n    let a = 1 let b = 2;\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "let b",
            after: (SyntaxKind::LetStmt, "let b = 2;"),
        },
        Fixture {
            what: "a missing right-hand side",
            text: "fn main() -> Int {\n    let mut x = 0;\n    x = ;\n    x = 2;\n    x\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ";\n    x = 2",
            after: (SyntaxKind::AssignStmt, "x = 2;"),
        },
        Fixture {
            what: "a missing closing parenthesis",
            text: "fn f(n: Int) -> Int { n }\nfn main() -> Int {\n    let a = f(1;\n    let b = f(2);\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ";\n    let b = f(2)",
            after: (SyntaxKind::CallExpr, "f(2)"),
        },
        Fixture {
            what: "a missing closing bracket in type arguments",
            text: "fn id[T](v: T) -> T { v }\nfn main() -> Int {\n    let a = id[Int(1);\n    let b = id[Int](2);\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "(1);",
            after: (SyntaxKind::LetStmt, "let b = id[Int](2);"),
        },
        Fixture {
            what: "a bad record construction",
            text: "struct P { x: Int, }\nfn main() -> Int {\n    let p = P(x 1);\n    let q = P(x: 2);\n    q.x\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "1);\n    let q",
            after: (SyntaxKind::ConstructExpr, "P(x: 2)"),
        },
        Fixture {
            what: "a bad enum construction",
            text: "enum E { A, B(v: Int), }\nfn main() -> Int {\n    let e = E.B(v: );\n    let f = E.A();\n    0\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ");\n    let f",
            after: (SyntaxKind::VariantExpr, "E.A()"),
        },
        Fixture {
            what: "a bad match arm",
            text: "enum E { A, B, }\nfn main() -> Int {\n    let n = match E.A() { E.A() 1, E.B() => 2, };\n    let m = 3;\n    m\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "1, E.B()",
            after: (SyntaxKind::LetStmt, "let m = 3;"),
        },
        Fixture {
            what: "a bad nested block",
            text: "fn main() -> Int {\n    let a = { let x = ; x };\n    let b = { let y = 2; y };\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "; x }",
            after: (SyntaxKind::LetStmt, "let b = { let y = 2; y };"),
        },
        Fixture {
            what: "an unexpected token between two valid statements",
            text: "fn main() -> Int {\n    let a = 1;\n    )\n    let b = 2;\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ")\n    let b",
            after: (SyntaxKind::LetStmt, "let b = 2;"),
        },
        Fixture {
            what: "a missing expression before a closing brace",
            text: "fn main() -> Int {\n    let a = 1;\n    while a < 2 { a = ; }\n    let b = 2;\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "; }\n    let b",
            after: (SyntaxKind::LetStmt, "let b = 2;"),
        },
        Fixture {
            what: "an error followed by the valid final expression",
            text: "fn main() -> Int {\n    let a = 1 +;\n    a * 2\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: ";\n    a * 2",
            after: (SyntaxKind::BinaryExpr, "a * 2"),
        },
        Fixture {
            what: "a missing variable name",
            text: "fn main() -> Int {\n    let = 5;\n    let b = 2;\n    b\n}\n",
            code: codes::UNEXPECTED_TOKEN,
            at: "= 5;",
            after: (SyntaxKind::LetStmt, "let b = 2;"),
        },
    ]
}

#[test]
fn each_malformed_fixture_is_one_diagnostic_one_error_region_and_the_rest_intact() {
    on_the_documented_stack(|| {
        for f in fixtures() {
            let (tree, diagnostics) = cst(f.text);
            assert_lossless(&tree, f.text, f.what);
            assert_eq!(diagnostics.len(), 1, "{}: {diagnostics:#?}", f.what);
            let d = &diagnostics[0];
            assert_eq!(d.code, f.code, "{}", f.what);
            let at = f
                .text
                .find(f.at)
                .unwrap_or_else(|| panic!("{}: fixture", f.what));
            assert_eq!(d.primary.span.start as usize, at, "{}: {d:#?}", f.what);
            assert!(kinds(&tree).contains(&SyntaxKind::Error), "{}", f.what);
            let (kind, after) = f.after;
            assert!(
                texts_of(&tree, kind).iter().any(|t| t == after),
                "{}: `{after}` is not a {kind:?} in the tree: {:#?}",
                f.what,
                tree.nodes()
                    .iter()
                    .map(|n| (n.kind(), n.text()))
                    .collect::<Vec<_>>()
            );
            // The whole function is still one function node: the error did not end it.
            let fns = texts_of(&tree, SyntaxKind::FnItem);
            assert!(
                fns.iter().any(|t| t.contains(after) && t.ends_with('}')),
                "{}: the function around the error is not whole",
                f.what
            );
        }
    });
}

#[test]
fn two_independent_mistakes_in_one_function_are_two_diagnostics() {
    let text = "fn main() -> Int {\n    let a = ;\n    let b = 2;\n    let c = * 3;\n    let d = 4;\n    b + d\n}\n";
    let (tree, diagnostics) = cst(text);
    assert_lossless(&tree, text, "two mistakes");
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert_eq!(
        texts_of(&tree, SyntaxKind::Error),
        ["let a = ;", "let c = * 3;"]
    );
    assert_eq!(
        texts_of(&tree, SyntaxKind::LetStmt),
        ["let b = 2;", "let d = 4;"]
    );
    assert_eq!(texts_of(&tree, SyntaxKind::BinaryExpr), ["b + d"]);
}

#[test]
fn one_mistaken_token_is_one_diagnostic_wherever_it_is() {
    // Missing `;`, `)`, `]`, `}`, an expression, a type and a name, each at a
    // representative boundary, and one token too many. One mistake, one report — recovery
    // must not turn it into a cascade about the statements after it.
    for (what, text) in [
        ("`;`", "fn main() -> Int { let a = 1 let b = 2; b }"),
        ("`)`", "fn main() -> Int { let a = (1 + 2; let b = 2; b }"),
        (
            "`]`",
            "fn id[T](v: T) -> T { v }\nfn main() -> Int { let a = id[Int(1); let b = 2; b }",
        ),
        (
            "`}` of a nested block",
            "fn main() -> Int { let a = { 1; let b = 2; b }\nfn g() -> Int { 1 }",
        ),
        (
            "`}` of a function",
            "fn main() -> Int { let b = 2; b\nfn g() -> Int { 1 }",
        ),
        (
            "an expression",
            "fn main() -> Int { let a = 1 + ; let b = 2; b }",
        ),
        ("a type", "fn f(x: ) -> Int { 1 }\nfn main() -> Int { 1 }"),
        ("a name", "fn main() -> Int { let = 1; let b = 2; b }"),
        (
            "a `(` of a call",
            "fn f(n: Int) -> Int { n }\nfn main() -> Int { let a = f 1); let b = 2; b }",
        ),
        // An extra `}` closes the function early, and what followed it is left at the top
        // level. That is still the one mistake: without the item's delimiters balancing,
        // the stray `else` there is not a second one.
        (
            "an extra `}`",
            "fn f(n: Int) -> Int { n }\nfn main() -> Int {\n    let t = f(if true { 1 }} else { 2 });\n    let u = 3;\n    u\n}\nfn g() -> Int { 1 }",
        ),
    ] {
        let (tree, diagnostics) = cst(text);
        assert_lossless(&tree, text, what);
        assert_eq!(diagnostics.len(), 1, "{what}: {diagnostics:#?}");
    }
}

#[test]
fn recovery_resumes_inside_the_block_it_started_in() {
    // The failed statement opened a `{` it did not close before failing. Skipping from
    // zero would take that `match`'s `}` for the block's and end the block early; the
    // statement after it would then be reported as a stray token at the top level.
    let text = "enum E { A, B, }\nfn main() -> Int {\n    let n = f(match E.A() { E.A() => 1 E.B() => 2 });\n    let m = 3;\n    m\n}\n";
    let (tree, diagnostics) = cst(text);
    assert_lossless(&tree, text, "an unclosed match");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(texts_of(&tree, SyntaxKind::LetStmt).contains(&"let m = 3;".to_owned()));
    assert_eq!(texts_of(&tree, SyntaxKind::FnItem).len(), 1);
}

#[test]
fn the_abstract_tree_keeps_its_contract_for_a_body_that_recovered() {
    // The concrete tree has every statement; the abstract tree has no body, exactly as it
    // had none before recovery existed. The checker never sees a statement list with a
    // hole in it, and the signature is still there for every caller.
    let text = "fn helper(n: Int) -> Int {\n    let broken = ;\n    n\n}\nfn main() -> Int { helper(1) }\n";
    let (program, tree, diagnostics) = parse_both(FileId::ROOT, text);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let helper = program
        .functions
        .iter()
        .find(|f| f.name == "helper")
        .expect("the signature is kept");
    assert!(helper.body.is_none());
    assert!(program
        .functions
        .iter()
        .any(|f| f.name == "main" && f.body.is_some()));
    assert_eq!(texts_of(&tree, SyntaxKind::NameRef), ["n"]);
}

#[test]
fn a_mistake_inside_a_body_does_not_reach_the_next_function() {
    let text = "fn a() -> Int {\n    let x = (1 +;\n    x\n}\nfn b() -> Int { 2 }\n";
    let (program, tree, diagnostics) = parse_both(FileId::ROOT, text);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(texts_of(&tree, SyntaxKind::FnItem).len(), 2);
    assert!(program
        .functions
        .iter()
        .any(|f| f.name == "b" && f.body.is_some()));
}

// ---------------------------------------------------------------------------------------
// Termination and bounds.
// ---------------------------------------------------------------------------------------

#[test]
fn recovery_always_makes_progress() {
    // Inputs built to stall a recovery that could stop without consuming: stray closers,
    // keywords that start nothing here, items inside bodies, a lone `pub`, nothing but
    // punctuation. Finishing is the assertion; the tree and the bytes are checked too.
    let inputs = [
        "fn main() -> Int { ) ) ) }",
        "fn main() -> Int { let }",
        "fn main() -> Int { let let let }",
        "fn main() -> Int { else else }",
        "fn main() -> Int { => , : . ? }",
        "}}}}",
        "))))",
        "pub pub pub",
        "pub",
        "fn",
        "struct",
        "enum",
        "use",
        "{",
        "fn main() -> Int {",
        "fn main() -> Int { fn inner() -> Int { 1 } }",
        "fn main() -> Int { struct S { x: Int } 1 }",
        "fn main() -> Int { pub fn f() -> Int { 1 } }",
        "fn main() -> Int { return; return; }",
        "fn main() -> Int { break break }",
        "fn main() -> Int { match { } }",
        "fn main() -> Int { if { } else }",
        "fn main() -> Int { scope scope }",
        "fn main(tasks: SpawnCap) -> Int { spawn spawn }",
        "; ; ;",
        "fn f[ -> Int { 1 }",
        "fn f( -> Int { 1 }",
    ];
    on_the_documented_stack(move || {
        for text in inputs {
            let (tree, _) = cst(text);
            assert_lossless(&tree, text, text);
        }
    });
}

#[test]
fn malformed_input_past_the_nesting_limit_is_bounded_and_kept() {
    // Deep and unclosed, so recovery runs at the bottom of a tree already at the limit.
    // The limit still decides, once, and the tree still holds every byte.
    let deep = MAX_NESTING * 2;
    let inputs = [
        format!("fn main() -> Int {{ {}1", "(".repeat(deep)),
        format!("fn main() -> Int {{ {}", "{ ".repeat(deep)),
        format!(
            "fn main() -> Int {{ {}1 ;; let b = 2; b }}",
            "- ".repeat(deep)
        ),
        format!(
            "fn main() -> Int {{ {} let x = ; x {} }}",
            "{ ".repeat(MAX_NESTING - 10),
            "} ".repeat(MAX_NESTING - 10)
        ),
    ];
    on_the_documented_stack(move || {
        for text in &inputs {
            let (tree, diagnostics) = cst(text);
            assert_lossless(&tree, text, "deep malformed input");
            assert!(!diagnostics.is_empty());
            let too_deep = diagnostics
                .iter()
                .filter(|d| d.code == codes::NESTING_TOO_DEEP)
                .count();
            assert!(too_deep <= 1, "{too_deep} nesting diagnostics");
        }
    });
}

// ---------------------------------------------------------------------------------------
// Properties, over seeded random input. Deterministic: the seed is fixed, so a failure
// reproduces, and the sizes are bounded, so the suite's time does not depend on luck.
// ---------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64: small, fixed, and enough to scatter token choices.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap_or(0)
    }
}

/// Source-shaped noise: real spellings of every token class, trivia of several kinds,
/// characters the lexer refuses, and unterminated strings — in any order.
fn soup(rng: &mut Rng, pieces: usize) -> String {
    const WORDS: &[&str] = &[
        "fn",
        "let",
        "mut",
        "if",
        "else",
        "while",
        "break",
        "continue",
        "return",
        "match",
        "struct",
        "enum",
        "pub",
        "use",
        "scope",
        "spawn",
        "true",
        "false",
        "main",
        "x",
        "Int",
        "Box",
        "E",
        "0",
        "1_000",
        "99999999999999999999",
        "\"s\"",
        "\"a b\"",
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        ".",
        ";",
        "?",
        "->",
        "=>",
        "=",
        "==",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "+",
        "-",
        "*",
        "/",
        "%",
        "@",
        "#",
        "é",
        "\"open\n",
        "// c\n",
        " ",
        "  ",
        "\t",
        "\n",
        "\r\n",
    ];
    let mut out = String::new();
    for _ in 0..pieces {
        out.push_str(WORDS[rng.below(WORDS.len())]);
        if rng.below(3) != 0 {
            out.push(' ');
        }
    }
    out
}

#[test]
fn random_source_shaped_input_always_round_trips_and_terminates() {
    on_the_documented_stack(|| {
        let mut rng = Rng(0x0015_c057_0000_0001);
        for case in 0..3000 {
            let pieces = 1 + rng.below(80);
            let text = soup(&mut rng, pieces);
            let (tree, _) = cst(&text);
            assert_lossless(&tree, &text, &format!("case {case}: {text:?}"));
        }
    });
}

#[test]
fn a_valid_program_with_one_token_removed_round_trips_and_keeps_its_first_diagnostic() {
    // Mutations of real programs rather than noise: every file of the compiler written in
    // Nazm, a token removed at seeded positions. The tree must still hold every byte, and
    // what the parser reports first must be what `parse_file` reports first for the same
    // text — they are one parse, so this is a check on the harness as much as the parser.
    let root = workspace();
    let files: Vec<String> = ["compiler/lex.nz", "compiler/parse.nz", "compiler/module.nz"]
        .iter()
        .map(|f| std::fs::read_to_string(root.join(f)).unwrap())
        .collect();
    on_the_documented_stack(move || {
        let mut rng = Rng(0x0015_7e57_0000_0002);
        for text in &files {
            let (tokens, _) = nazm_syntax::lex(text);
            for _ in 0..60 {
                let t = &tokens[rng.below(tokens.len() - 1)];
                let (a, b) = (t.span.start as usize, t.span.end as usize);
                let mutated = format!("{}{}", &text[..a], &text[b..]);
                let (tree, diagnostics) = cst(&mutated);
                assert_lossless(&tree, &mutated, "a token removed");
                let (_, file_diagnostics) = nazm_syntax::parse_file(FileId::ROOT, &mutated);
                assert_eq!(diagnostics, file_diagnostics);
            }
        }
    });
}

// ---------------------------------------------------------------------------------------
// The substrate for structured edits, proven narrowly.
// ---------------------------------------------------------------------------------------

#[test]
fn a_node_is_found_by_its_position_and_reproduced_from_its_own_leaves() {
    // Find the statement that contains a given name, through the tree's structure alone,
    // reproduce it from its leaves, and replace exactly its bytes. No text search decides
    // where the statement begins or ends.
    let text = "fn main() -> Int {\n    let a = 1;\n    // keep me\n    let b = a + 2;\n    b\n}\n";
    let (tree, _) = cst(text);
    let b_leaf = tree
        .leaves()
        .into_iter()
        .find(|l| l.kind == SyntaxKind::Ident && l.text == "b")
        .unwrap();
    let covering = tree.root().covering(b_leaf.span).unwrap();
    assert_eq!(
        covering.kind(),
        SyntaxKind::LetStmt,
        "the smallest node over `b`"
    );
    let statement = covering.span();
    assert_eq!(covering.text(), "let b = a + 2;");
    assert_eq!(
        text.get(statement.start as usize..statement.end as usize),
        Some(covering.text().as_str())
    );

    // Replace exactly those bytes, and nothing else in the file moves.
    let edited = format!(
        "{}let b = a * 3;{}",
        &text[..statement.start as usize],
        &text[statement.end as usize..]
    );
    let (after, diagnostics) = cst(&edited);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_lossless(&after, &edited, "after the edit");
    assert_eq!(
        texts_of(&after, SyntaxKind::LetStmt),
        ["let a = 1;", "let b = a * 3;"]
    );
    let comments = |t: &Cst| -> Vec<(u32, String)> {
        t.leaves()
            .into_iter()
            .filter(|l| l.kind == SyntaxKind::Comment)
            .map(|l| (l.span.start, l.text.to_owned()))
            .collect()
    };
    assert_eq!(
        comments(&tree),
        comments(&after),
        "the comment before it did not move"
    );
}

#[test]
fn an_effect_set_is_one_node_kept_byte_for_byte_and_a_broken_one_costs_nothing_after_it() {
    // N36. The set is a node of its own, with its `!`, braces, names and commas, however
    // it was spaced.
    let text = "fn f() -> Int ! { io ,\n spawn } { 1 }\nfn g() -> Int !{} { 2 }\nfn main() -> Int { f() + g() }\n";
    let (tree, errors) = cst(text);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_lossless(&tree, text, "effect sets");
    assert_eq!(
        texts_of(&tree, SyntaxKind::EffectSet),
        ["! { io ,\n spawn }", "!{}"]
    );
    // Malformed sets: each is one error, every byte is kept, and the next function is
    // still a function.
    for broken in [
        "fn f() -> Int ! { io, } { 1 }\n",
        "fn f() -> Int ! io { 1 }\n",
        "fn f() -> Int ! { 3 } { 1 }\n",
        "fn f() -> Int ! { io spawn } { 1 }\n",
        "fn f() -> Int ! { io  { 1 }\n",
        "fn f() -> Int ! \n",
    ] {
        let text = format!("{broken}fn g() -> Int {{ 2 }}\n");
        let (tree, errors) = cst(&text);
        assert!(!errors.is_empty(), "{broken:?}");
        assert!(
            errors
                .iter()
                .all(|e| e.code == codes::UNEXPECTED_TOKEN || e.code.as_str().starts_with("N01")),
            "{broken:?}: {errors:#?}"
        );
        assert_lossless(&tree, &text, broken);
        assert!(
            texts_of(&tree, SyntaxKind::FnItem)
                .iter()
                .any(|t| t == "fn g() -> Int { 2 }"),
            "{broken:?}: {:?}",
            texts_of(&tree, SyntaxKind::FnItem)
        );
    }
}

#[test]
fn a_parse_without_the_tree_is_the_parse_with_it_on_every_file() {
    // `parse_file` builds no lossless tree (performance leadership, `nazm check`): the same
    // scan and the same parser, the abstract tree and the diagnostics the same as
    // `parse_both`'s for every `.nz` file in the repository, the malformed ones included.
    let files = corpus();
    assert!(files.len() >= 190, "only {} .nz files found", files.len());
    on_the_documented_stack(move || {
        for (name, text) in &files {
            let (with, _, with_errors) = parse_both(FileId::ROOT, text);
            let (without, without_errors) = nazm_syntax::parse_file(FileId::ROOT, text);
            assert!(with == without, "{name}: the abstract trees differ");
            assert_eq!(with_errors, without_errors, "{name}");
        }
    });
}

#[test]
fn a_kept_parse_stands_for_its_own_text_only() {
    // `keep` hands a loader's parse to the next `parse_map` of the same file id and text; any
    // other text under that id is parsed afresh, and a kept parse is taken once.
    on_the_documented_stack(|| {
        let id = FileId::from_index(7);
        let a = "fn f() -> Int { 1 }\n";
        let b = "fn g() -> Int { 2 }\n";
        let pad: Vec<(String, String)> = (0..7)
            .map(|i| (format!("pad{i}.nz"), String::new()))
            .collect();
        let map_of = |text: &str| {
            let mut parts = pad.clone();
            parts.push(("x.nz".to_owned(), text.to_owned()));
            nazm_span::SourceMap::from_parts(parts)
        };
        let (pa, ea) = nazm_syntax::parse_file(id, a);
        nazm_syntax::keep(id, a, pa.clone(), ea);
        // Another text under the same id: its own parse, not the kept one.
        let (from_b, _) = nazm_syntax::parse_map(&map_of(b));
        assert_eq!(
            from_b
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["g"]
        );
        // The kept text: the kept parse, the same as a fresh one.
        let (from_a, _) = nazm_syntax::parse_map(&map_of(a));
        assert!(from_a == pa);
        let (again, _) = nazm_syntax::parse_map(&map_of(a));
        assert!(again == pa);
    });
}
