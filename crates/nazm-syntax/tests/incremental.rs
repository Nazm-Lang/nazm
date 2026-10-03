//! Incremental reparse (N76): every revision an edit produces is a fresh parse's equal.
//!
//! The oracle is [`Revision::new`] of the edited text, compared field by field: the
//! lossless scan, every node and leaf of the concrete tree with its kind, span and bytes,
//! the abstract tree with every span, and the diagnostics in order. Seeded edit sequences
//! over every `.nz` file in the repository, the grammar's own constructs, and the edits
//! that are hard on purpose: an edit at a chunk's first byte, one that merges two tokens,
//! one that opens a delimiter nothing closes, one inside a string or a comment.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_span::{FileId, Span};
use nazm_syntax::{
    parse_both, Cst, Lexeme, ParseStats, Revision, SyntaxKind, TextEdit, PARSE_STACK_BYTES,
};

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

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

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
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

/// Everything a tree says, in preorder: each node's kind and span, each leaf's kind, span
/// and bytes.
type Shape = (Vec<(SyntaxKind, Span)>, Vec<(SyntaxKind, Span, String)>);

fn shape(cst: &Cst) -> Shape {
    let nodes = cst.nodes().iter().map(|n| (n.kind(), n.span())).collect();
    let leaves = cst
        .leaves()
        .iter()
        .map(|l| (l.kind, l.span, l.text.to_owned()))
        .collect();
    (nodes, leaves)
}

/// The revision equals a fresh parse of its own text, in every observable.
fn assert_converged(r: &Revision, what: &str) {
    let fresh = Revision::new(r.file(), r.text().to_owned());
    assert_eq!(r.lexemes(), fresh.lexemes(), "{what}: lexemes");
    assert_eq!(shape(r.cst()), shape(fresh.cst()), "{what}: concrete tree");
    assert_eq!(r.cst().text(), r.text(), "{what}: lossless");
    assert_eq!(r.program(), fresh.program(), "{what}: abstract tree");
    assert_eq!(r.diagnostics(), fresh.diagnostics(), "{what}: diagnostics");
    // And the revision's own full parse is the one every command runs.
    let (program, cst, diagnostics) = parse_both(r.file(), r.text());
    assert_eq!(program, fresh.program(), "{what}: parse_both program");
    assert_eq!(shape(&cst), shape(fresh.cst()), "{what}: parse_both tree");
    assert_eq!(
        diagnostics,
        fresh.diagnostics(),
        "{what}: parse_both diagnostics"
    );
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next() % (n as u64)).unwrap()
        }
    }
}

/// Fragments an author types: delimiters, keywords, a whole item, a string's and a
/// comment's openers, a non-ASCII character, and a backslash.
const FRAGMENTS: &[&str] = &[
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ";",
    ",",
    "\"",
    "//",
    "\n",
    " ",
    "fn",
    "let x = 1;",
    "fn extra() -> Int { 0 }\n",
    "struct S { a: Int, }\n",
    "enum E { A, B(x: Int), }\n",
    "use \"other.nz\";\n",
    "pub ",
    "é",
    "\\",
    "x",
    "1",
    "->",
    "=>",
    "::",
    "&&",
    "\\u{e9}",
    "match",
    "if",
    "else",
    "while",
    "return",
];

fn floor_boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn random_edit(rng: &mut Rng, text: &str) -> TextEdit {
    let start = floor_boundary(text, rng.below(text.len() + 1));
    let kind = rng.below(4);
    let end = if kind == 0 {
        start
    } else {
        floor_boundary(text, start + rng.below(24))
    };
    let insert = match kind {
        0 | 2 => FRAGMENTS[rng.below(FRAGMENTS.len())].to_owned(),
        // A slice of the file itself, moved elsewhere: realistic text, often unbalanced.
        3 => {
            let a = floor_boundary(text, rng.below(text.len() + 1));
            let b = floor_boundary(text, a + rng.below(40));
            text[a..b].to_owned()
        }
        _ => String::new(),
    };
    TextEdit {
        start: u32::try_from(start).unwrap(),
        end: u32::try_from(end).unwrap(),
        text: insert,
    }
}

#[test]
fn seeded_edit_sequences_converge_to_a_fresh_parse_over_the_whole_repository() {
    on_the_documented_stack(|| {
        let files = corpus();
        assert!(
            files.len() > 100,
            "the corpus is the repository: {}",
            files.len()
        );
        let mut steps = 0usize;
        for (k, (name, text)) in files.iter().enumerate() {
            // Big files get fewer steps: each step's oracle parses the whole file.
            let budget = if text.len() > 50_000 { 3 } else { 12 };
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (k as u64 + 1));
            let mut r = Revision::new(FileId::ROOT, text.clone());
            for step in 0..budget {
                let edit = random_edit(&mut rng, r.text());
                r = r.edit(&edit).unwrap();
                assert!(
                    !r.stats().full,
                    "{name}: an edit is never a silent full parse"
                );
                assert_converged(&r, &format!("{name} step {step} {edit:?}"));
                steps += 1;
            }
        }
        assert!(steps > 1_000, "{steps} steps");
    });
}

#[test]
fn edits_at_the_hard_places_converge() {
    let base = "use \"a.nz\";\n// lead\nfn a() -> Int { 1 }\n\nstruct S { x: Int, }\nfn b() -> Int { \"s\" }\nenum E { A, }\n// tail\n";
    let at = |s: &str| u32::try_from(base.find(s).unwrap()).unwrap();
    let edit = |start: u32, end: u32, text: &str| TextEdit {
        start,
        end,
        text: text.to_owned(),
    };
    let cases = vec![
        // At the first byte of an item: the token before may merge with what is typed.
        edit(at("fn a"), at("fn a"), "x"),
        edit(at("fn b") - 1, at("fn b") - 1, "y"),
        // Two tokens become one.
        edit(at("() -> Int { 1"), at("() -> Int { 1"), "zz"),
        // A delimiter nothing closes: the reparse runs to the end of the file.
        edit(at("{ 1 }"), at("{ 1 }"), "{"),
        // A delimiter removed.
        edit(at("{ x"), at("{ x") + 1, ""),
        // Inside a string, and opening one that does not close.
        edit(at("\"s\"") + 1, at("\"s\"") + 1, "abc"),
        edit(at("\"s\""), at("\"s\"") + 1, ""),
        // Into a comment, and a comment opened over an item.
        edit(at("// lead") + 3, at("// lead") + 3, "more"),
        edit(at("fn b"), at("fn b"), "// "),
        // The very end and the very start.
        edit(
            u32::try_from(base.len()).unwrap(),
            u32::try_from(base.len()).unwrap(),
            "fn z() -> Int { 0 }",
        ),
        edit(0, 0, "\n"),
        edit(0, u32::try_from(base.len()).unwrap(), ""),
        // A whole item deleted, and one duplicated.
        edit(at("struct"), at("fn b"), ""),
        edit(at("fn a"), at("fn a"), "fn a() -> Int { 1 }\n"),
        // An escape typed into a literal: valid, then refused.
        edit(at("\"s\"") + 1, at("\"s\"") + 2, "\\u{1F600}"),
        edit(at("\"s\"") + 1, at("\"s\"") + 2, "\\u{D800}"),
    ];
    on_the_documented_stack(|| {
        let r = Revision::new(FileId::ROOT, base);
        for e in cases {
            let next = r.edit(&e).unwrap();
            assert_converged(&next, &format!("{e:?}"));
            // And the same edit applied to the result of another edit.
            let again = next
                .edit(&TextEdit {
                    start: 0,
                    end: 0,
                    text: " ".to_owned(),
                })
                .unwrap();
            assert_converged(&again, &format!("{e:?} then a space"));
        }
    });
}

#[test]
fn an_edit_inside_one_function_of_a_large_file_reparses_that_function_alone() {
    on_the_documented_stack(|| {
        let text = std::fs::read_to_string(workspace().join("compiler/emit.nz")).unwrap();
        let r = Revision::new(FileId::ROOT, text.clone());
        let full: ParseStats = r.stats();
        assert!(full.full && full.chunks_reparsed > 50, "{full:?}");
        // Inside the body of some function in the middle of the file: rename a local.
        let middle = text.len().div_euclid(2);
        let at = middle + text[middle..].find("let ").unwrap() + 4;
        let next = r
            .edit(&TextEdit {
                start: u32::try_from(at).unwrap(),
                end: u32::try_from(at).unwrap(),
                text: "renamed_".to_owned(),
            })
            .unwrap();
        let s = next.stats();
        assert!(!s.full);
        assert_eq!(s.chunks_reparsed, 1, "{s:?}");
        assert_eq!(s.chunks_reused + 1, full.chunks_reparsed, "{s:?}");
        assert!(s.tokens_reparsed * 20 < s.tokens_total, "{s:?}");
        assert!(s.lexemes_relexed <= 3, "{s:?}");
        assert_converged(&next, "emit.nz one-word edit");
    });
}

#[test]
fn a_one_literal_edit_reuses_the_items_around_it() {
    on_the_documented_stack(|| {
        let text = "fn a() -> Int { 1 }\nfn b() -> Int { 2 }\nfn c() -> Int { 3 }\n";
        let r = Revision::new(FileId::ROOT, text);
        let at = u32::try_from(text.find('2').unwrap()).unwrap();
        let next = r
            .edit(&TextEdit {
                start: at,
                end: at + 1,
                text: "20".to_owned(),
            })
            .unwrap();
        let s = next.stats();
        // `fn b` reparsed; `fn a` before it and `fn c` after it kept. Their green nodes are
        // the previous tree's by construction (`Revision::edit`), and equal by this test.
        assert_eq!((s.chunks_reparsed, s.chunks_reused), (1, 2), "{s:?}");
        // The literal, and the space before it: its end is where the edit begins, and the
        // lexer read one byte past it.
        assert_eq!(s.lexemes_relexed, 2, "{s:?}");
        assert_converged(&next, "one literal");
    });
}

#[test]
fn a_delimiter_left_open_reparses_to_the_end_and_still_converges() {
    on_the_documented_stack(|| {
        let mut text = String::new();
        for i in 0..40 {
            text.push_str(&format!("fn f{i}() -> Int {{ {i} }}\n"));
        }
        let r = Revision::new(FileId::ROOT, text.clone());
        let at = u32::try_from(text.find("fn f3()").unwrap()).unwrap();
        let next = r
            .edit(&TextEdit {
                start: at,
                end: at,
                text: "fn open() -> Int { (".to_owned(),
            })
            .unwrap();
        assert_converged(&next, "an open delimiter");
        assert!(!next.diagnostics().is_empty());
        // And closing it again brings back reuse.
        let close = next
            .edit(&TextEdit {
                start: at + 20,
                end: at + 20,
                text: ") }\n".to_owned(),
            })
            .unwrap();
        assert_converged(&close, "closed again");
        assert!(close.stats().chunks_reused > 30, "{:?}", close.stats());
    });
}

#[test]
fn the_lexemes_of_a_revision_cover_its_text() {
    on_the_documented_stack(|| {
        let r = Revision::new(FileId::ROOT, "fn a() -> Int { 1 }");
        let next = r
            .edit(&TextEdit {
                start: 3,
                end: 4,
                text: "bb".to_owned(),
            })
            .unwrap();
        let mut at = 0;
        for l in next.lexemes() {
            assert_eq!(l.span().start, at);
            at = l.span().end;
            if matches!(l, Lexeme::Token(_)) {
                assert!(l.span().end as usize <= next.text().len());
            }
        }
        assert_eq!(at as usize, next.text().len());
    });
}

#[test]
fn an_edit_past_an_item_that_its_lookahead_read_reparses_that_item() {
    // `fn ,` does not parse, and recovery skips to the next `fn NAME` — reading the name
    // after the next `fn`, which is the next item's. Replacing that name with something
    // that is not one changes where the broken item ends, so it must be reparsed although
    // the edit is not in it. Found by the seeded sequences (`conflicting-inference.nz`).
    let text = "fn , b: T) -> T { a }\n\nfn main() -> Int { 1 }\nfn c() -> Int { 2 }\n";
    on_the_documented_stack(|| {
        let r = Revision::new(FileId::ROOT, text);
        let at = u32::try_from(text.find("main").unwrap()).unwrap();
        for insert in ["struct S { a: Int, }\n", "(", "1", "pub", ""] {
            let next = r
                .edit(&TextEdit {
                    start: at,
                    end: at + 4,
                    text: insert.to_owned(),
                })
                .unwrap();
            assert_converged(&next, insert);
        }
    });
}

/// The measurement `performance.md` reports (N76): a full parse of each compiler source
/// against a one-word edit in the middle of it, median of 31 after 5 warm-ups. Ignored: it
/// is a timing, run by hand in a release build —
/// `cargo test --release -p nazm-syntax --test incremental -- --ignored --nocapture`.
#[test]
#[ignore = "a release-build timing, run by hand"]
fn measure_full_parse_against_one_edit() {
    fn median(mut v: Vec<u128>) -> u128 {
        v.sort_unstable();
        v[v.len().div_euclid(2)]
    }
    on_the_documented_stack(|| {
        for name in [
            "compiler/emit.nz",
            "compiler/analyse.nz",
            "compiler/parse.nz",
        ] {
            let text = std::fs::read_to_string(workspace().join(name)).unwrap();
            let middle = text.len().div_euclid(2);
            let at = u32::try_from(middle + text[middle..].find("let ").unwrap() + 4).unwrap();
            let edit = TextEdit {
                start: at,
                end: at,
                text: "x".to_owned(),
            };
            let base = Revision::new(FileId::ROOT, text.clone());
            let mut full = Vec::new();
            let mut inc = Vec::new();
            let mut parse_both_times = Vec::new();
            for i in 0..36 {
                let t = std::time::Instant::now();
                let r = Revision::new(FileId::ROOT, text.clone());
                let f = t.elapsed().as_micros();
                let t = std::time::Instant::now();
                let e = base.edit(&edit).unwrap();
                let n = t.elapsed().as_micros();
                let t = std::time::Instant::now();
                let p = parse_both(FileId::ROOT, &text);
                let pb = t.elapsed().as_micros();
                assert!(r.stats().full && !e.stats().full);
                drop((r, e, p));
                if i >= 5 {
                    full.push(f);
                    inc.push(n);
                    parse_both_times.push(pb);
                }
            }
            let e = base.edit(&edit).unwrap();
            println!(
                "{name}: {} bytes; parse_both {} us, Revision::new {} us, one edit {} us; \
                 relexed {} of {} lexemes, reparsed {} of {} tokens, {} of {} chunks reused",
                text.len(),
                median(parse_both_times),
                median(full),
                median(inc),
                e.stats().lexemes_relexed,
                e.stats().lexemes_total,
                e.stats().tokens_reparsed,
                e.stats().tokens_total,
                e.stats().chunks_reused,
                e.stats().chunks_reused + e.stats().chunks_reparsed,
            );
        }
    });
}

#[test]
fn a_revision_is_the_roots_parse_only_when_it_is_of_the_roots_text() {
    on_the_documented_stack(|| {
        let text = "fn main() -> Int { 1 }\n";
        let map = nazm_span::SourceMap::single("main.nz".to_owned(), text.to_owned());
        let same = Revision::new(FileId::ROOT, text);
        let (p, d, used) = nazm_syntax::parse_map_with_root(&map, Some(&same));
        assert!(used);
        assert_eq!((p, d), nazm_syntax::parse_map(&map));
        // A revision of other text — an editor's copy that has moved on — is not used: the
        // root is parsed from the map's text, and the answer is that text's.
        let stale = Revision::new(FileId::ROOT, "fn main() -> Int { 2 }\n");
        let (p, d, used) = nazm_syntax::parse_map_with_root(&map, Some(&stale));
        assert!(!used);
        assert_eq!((p, d), nazm_syntax::parse_map(&map));
        let (_, _, used) = nazm_syntax::parse_map_with_root(&map, None);
        assert!(!used);
    });
}
