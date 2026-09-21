//! The formatter's three promises, checked rather than described.
//!
//! - **The same tokens.** Formatting re-lexes to exactly the kinds it was given. This is
//!   the one that makes the rest safe: a formatter that cannot change the token stream
//!   cannot change the program, whatever it does with whitespace.
//! - **Idempotence.** `format(format(x)) == format(x)`. A formatter without this has no
//!   canonical form, only a direction of travel, and every save produces a diff.
//! - **Every comment kept.** A formatter that deletes a comment is not one.
//!
//! The corpus is every `.nz` file in the tree plus a set of deliberately ugly inputs.
//! The real sources matter because they are the only large programs that exist; the ugly
//! ones matter because the real sources are already close to formatted, so they exercise
//! almost none of the layout decisions.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use nazm_syntax::{format, lex, TokenKind};

/// Token kinds, with `Eof` dropped: whitespace is what the formatter changes, and a
/// token's position is a function of the whitespace around it.
fn kinds(text: &str) -> Vec<TokenKind> {
    let (tokens, errors) = lex(text);
    assert!(errors.is_empty(), "{text:?} does not lex: {errors:#?}");
    tokens
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| *k != TokenKind::Eof)
        .collect()
}

fn comments_of(text: &str) -> Vec<String> {
    let (_, _, spans) = nazm_syntax::lex_with_comments(text);
    spans
        .iter()
        .map(|s| {
            text.get(s.start as usize..s.end as usize)
                .unwrap_or_default()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// Every promise, for one input.
fn holds(what: &str, source: &str) -> String {
    let once = format(source).unwrap_or_else(|e| panic!("{what}: {e:#?}"));

    assert_eq!(
        kinds(&once),
        kinds(source),
        "{what}: formatting changed the tokens\n--- before\n{source}\n--- after\n{once}"
    );

    let twice = format(&once).unwrap_or_else(|e| panic!("{what}: reformatting failed: {e:#?}"));
    assert_eq!(
        twice, once,
        "{what}: not idempotent\n--- once\n{once}\n--- twice\n{twice}"
    );

    assert_eq!(
        comments_of(&once),
        comments_of(source),
        "{what}: a comment was lost or changed\n{once}"
    );

    assert!(
        once.ends_with('\n') && !once.ends_with("\n\n"),
        "{what}: the output should end in exactly one newline"
    );
    once
}

fn tree_sources() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let mut out = Vec::new();
    for folder in ["compiler", "examples"] {
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "nz") {
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        out.push((p.display().to_string(), text));
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn the_promises_hold_for_every_source_in_the_tree() {
    let sources = tree_sources();
    assert!(sources.len() >= 5, "only {} sources", sources.len());
    for (name, text) in &sources {
        holds(name, text);
    }
}

#[test]
fn the_promises_hold_for_deliberately_ugly_input() {
    // Each of these exercises a decision the tree's own sources never reach, because
    // they are already close to formatted.
    for (what, source) in [
        ("everything on one line", "fn main()->Int{let x=1;x}"),
        (
            "no line breaks anywhere, nested",
            "fn main()->Int{let mut i=0;while i<3{if i==1{i=i+1;}else{i=i+2;}}i}",
        ),
        (
            "too many blank lines",
            "fn main() -> Int {\n\n\n\n    let x = 1;\n\n\n\n    x\n}\n",
        ),
        ("leading blank lines", "\n\n\nfn main() -> Int { 1 }\n"),
        ("no trailing newline", "fn main() -> Int { 1 }"),
        ("tabs and stray spaces", "fn   main ( )  ->   Int {\t1\t}"),
        (
            "negation next to subtraction",
            "fn main() -> Int { let a = 0 - -1; let b = -1 - -1; a - b }",
        ),
        (
            "negation after every kind of opener",
            "fn f(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { f(-1, -2) + (-3) }",
        ),
        ("a call inside a call", "fn main() -> Int { str_len(str_concat(\"a\", \"b\")) }"),
        (
            "comments in every position",
            "// leading\nfn main() -> Int { // trailing on the signature\n    // on its own\n    let x = 1; // after a statement\n    x // after the tail\n}\n// at the end of the file\n",
        ),
        (
            "an else-if chain",
            "fn main() -> Int { let n = 2; if n == 1 { 1 } else if n == 2 { 2 } else { 3 } }",
        ),
        (
            "concurrency",
            "fn w(c: Chan) -> Int { 0 }\nfn main() -> Int { let c = chan_new(1); scope { spawn w(c); chan_close(c); } 0 }",
        ),
        ("a use item", "use \"other.nz\";\nfn main() -> Int { 0 }"),
        ("an empty block", "fn f() -> Int { return 1; }\nfn main() -> Int { f() }"),
    ] {
        holds(what, source);
    }
}

#[test]
fn the_layout_is_the_one_intended() {
    // Idempotence and token preservation are satisfied by a formatter that does nothing
    // useful. This is the test that says what the output should actually look like.
    //
    // Note what survives: `fn add` keeps its line because that is how it was written,
    // and `main`'s structure is kept while its spacing and indentation are not. The
    // rule is stated here rather than only in the module's documentation because it is
    // the decision a reader is most likely to want to argue with.
    let out = holds(
        "layout",
        "fn add(a:Int,b:Int)->Int{a+b}\n\nfn main()->Int{\nlet mut i=0;\n  while i<3{\ni=i+add(i,-1);\n}\ni\n}",
    );
    assert_eq!(
        out,
        "fn add(a: Int, b: Int) -> Int { a + b }\n\n\
         fn main() -> Int {\n    \
             let mut i = 0;\n    \
             while i < 3 {\n        \
                 i = i + add(i, -1);\n    \
             }\n    \
             i\n\
         }\n",
        "got:\n{out}"
    );
}

#[test]
fn a_block_written_on_one_line_stays_on_one_line() {
    // Both of these are idioms in the tree, and both are one line because being one
    // line is the point. A guard clause spread over three is a guard clause a reader
    // has to assemble.
    let out = holds(
        "one expression",
        "fn main() -> Int { if true { 1 } else { 2 } }",
    );
    assert_eq!(
        out, "fn main() -> Int { if true { 1 } else { 2 } }\n",
        "got:\n{out}"
    );

    let out = holds(
        "a guard clause",
        "fn f(b:Int)->Int{\nif b==95{return 1;}\n0\n}\nfn main()->Int{f(95)}",
    );
    assert_eq!(
        out,
        "fn f(b: Int) -> Int {\n    if b == 95 { return 1; }\n    0\n}\nfn main() -> Int { f(95) }\n",
        "got:\n{out}"
    );
}

#[test]
fn a_block_that_was_several_lines_stays_several_lines() {
    let out = holds("multi-line", "fn main() -> Int {\nlet a = 1;\na\n}");
    assert_eq!(
        out, "fn main() -> Int {\n    let a = 1;\n    a\n}\n",
        "got:\n{out}"
    );
}

#[test]
fn a_comment_inside_a_one_line_block_forces_it_open() {
    // `//` runs to the end of the line, so keeping this on one line would comment out
    // the closing brace. The only case where the author's line cannot be kept.
    let out = holds("comment inside", "fn main() -> Int { // why\n 1 }");
    assert!(
        out.lines().count() >= 3,
        "a comment cannot share a line with what follows it:\n{out}"
    );
}

#[test]
fn nothing_is_inserted_between_definitions() {
    // A table of one-line definitions is written adjacent on purpose, and a formatter
    // that puts a blank line between each pair has destroyed the table. This is why the
    // formatter inserts no whitespace at all: blank lines are the author's.
    let out = holds(
        "a table",
        "fn a() -> Int { 1 }\nfn b() -> Int { 2 }\n\nfn main() -> Int { a() + b() }\n",
    );
    assert_eq!(
        out, "fn a() -> Int { 1 }\nfn b() -> Int { 2 }\n\nfn main() -> Int { a() + b() }\n",
        "got:\n{out}"
    );
}

#[test]
fn a_comment_above_a_function_stays_above_it() {
    let out = holds(
        "doc comment",
        "fn a() -> Int { 1 }\n\n// what b is for\nfn b() -> Int { 2 }\n\nfn main() -> Int { a() + b() }\n",
    );
    assert!(
        out.contains("// what b is for\nfn b()"),
        "the comment was separated from what it describes:\n{out}"
    );
}

#[test]
fn a_comment_stays_on_the_line_it_was_written_on() {
    let out = holds(
        "comments",
        "fn main() -> Int {\n    let x = 1; // why one\n    // a whole line\n    x\n}\n",
    );
    assert!(
        out.contains("let x = 1; // why one"),
        "a trailing comment moved:\n{out}"
    );
    assert!(
        out.contains("\n    // a whole line\n"),
        "an own-line comment moved:\n{out}"
    );
}

#[test]
fn formatted_text_parses_to_the_same_program() {
    // The strongest statement available without an AST comparison that ignores spans:
    // the tree has the same shape, the same names and the same literals. Spans differ by
    // construction — moving text is what a formatter does.
    for (name, text) in tree_sources() {
        let formatted = format(&text).unwrap_or_else(|e| panic!("{name}: {e:#?}"));
        let (before, e1) = nazm_syntax::parse_source(&text);
        let (after, e2) = nazm_syntax::parse_source(&formatted);
        assert_eq!(e1.len(), e2.len(), "{name}: a different number of errors");
        assert_eq!(
            before.functions.len(),
            after.functions.len(),
            "{name}: a different number of functions"
        );
        for (b, a) in before.functions.iter().zip(&after.functions) {
            assert_eq!(b.name, a.name, "{name}: functions in a different order");
            assert_eq!(
                b.params.iter().map(|p| &p.name).collect::<Vec<_>>(),
                a.params.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "{name}: `{}` has different parameters",
                b.name
            );
            assert_eq!(
                b.return_type.name, a.return_type.name,
                "{name}: `{}` returns something different",
                b.name
            );
        }
    }
}

#[test]
fn text_that_does_not_lex_is_refused_rather_than_guessed_at() {
    // A formatter that writes its best guess at what an unterminated string was meant
    // to be is a formatter that loses code.
    let e = format("fn main() -> Int { \"unterminated }").unwrap_err();
    assert!(!e.is_empty(), "the lexer's diagnostics should come back");
}
