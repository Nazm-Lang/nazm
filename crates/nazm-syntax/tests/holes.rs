//! A completion probe's parse (N26): the ordinary parse wherever there is no hole, and at a
//! hole a change to nothing but the statement holding it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_span::FileId;
use nazm_syntax::{parse_both, parse_both_at, Cst, HoleSite, SyntaxKind, PARSE_STACK_BYTES};

fn on_the_parse_stack(f: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join");
    });
}

/// A concrete tree as a list of every node's kind and extent, in order, and its text.
fn shape(cst: &Cst) -> (Vec<(SyntaxKind, u32, u32)>, String) {
    let root = cst.root();
    let mut nodes = vec![(root.kind(), root.span().start, root.span().end)];
    nodes.extend(
        root.descendants()
            .into_iter()
            .map(|n| (n.kind(), n.span().start, n.span().end)),
    );
    (nodes, cst.text())
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Every `.nz` file of the repository — the clean compiler and library, and the deliberately
/// broken programs the CLI's tests read — sorted.
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
    for dir in ["compiler", "library", "examples", "crates/nazm-cli/tests"] {
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

#[test]
fn a_probe_parse_is_the_ordinary_parse_wherever_there_is_no_hole() {
    on_the_parse_stack(a_probe_parse_is_the_ordinary_parse_wherever_there_is_no_hole_inner);
}

fn a_probe_parse_is_the_ordinary_parse_wherever_there_is_no_hole_inner() {
    // For every file, at offsets where the grammar has no hole — the start, the end, and
    // the end of tokens sampled through the file, `.` and `(` among them wherever a name or
    // a label follows — the probe's parse is the ordinary one: the same abstract tree with no
    // hole, the same concrete tree, the same diagnostics.
    let mut compared = 0;
    for (name, text) in corpus() {
        let (program, cst, diagnostics) = parse_both(FileId::ROOT, &text);
        let expected = shape(&cst);
        let ends: Vec<u32> = cst
            .root()
            .leaves()
            .into_iter()
            .filter(|l| !l.kind.is_trivia())
            .map(|l| l.span.end)
            .collect();
        let step = ends.len().div_ceil(24).max(1);
        let mut offsets: Vec<u32> = ends.into_iter().step_by(step).collect();
        offsets.push(0);
        offsets.push(u32::try_from(text.len()).unwrap());
        for at in offsets {
            let (p, c, d) = parse_both_at(FileId::ROOT, &text, Some(at));
            if p.hole.is_some() {
                // A real hole in a broken test program: judged by the next test.
                continue;
            }
            assert_eq!(p, program, "{name} @ {at}");
            assert_eq!(d, diagnostics, "{name} @ {at}");
            assert_eq!(shape(&c), expected, "{name} @ {at}");
            compared += 1;
        }
    }
    assert!(compared > 1000, "{compared}");
}

const DEFS: &str = "struct Point {\n    x: Int,\n}\n";

fn at_end_of(text: &str, needle: &str) -> u32 {
    u32::try_from(text.find(needle).unwrap() + needle.len()).unwrap()
}

#[test]
fn a_hole_is_read_only_at_the_cursor_and_changes_only_its_statement() {
    on_the_parse_stack(a_hole_is_read_only_at_the_cursor_and_changes_only_its_statement_inner);
}

fn a_hole_is_read_only_at_the_cursor_and_changes_only_its_statement_inner() {
    let text =
        format!("{DEFS}fn f(p: Point) -> Int {{\n    let n = p.\n    let z = 1;\n    z\n}}\n");
    let hole = at_end_of(&text, "p.");
    // The ordinary parse: one error, and the function has no body.
    let (plain, _, errors) = parse_both(FileId::ROOT, &text);
    assert_eq!(errors.len(), 1);
    assert!(plain.functions[0].body.is_none());
    // The probe: no error, the body kept — its first statement ends at the hole — and the
    // concrete tree still exactly the text.
    let (probe, cst, errors) = parse_both_at(FileId::ROOT, &text, Some(hole));
    assert_eq!(errors, []);
    let body = probe.functions[0].body.as_ref().unwrap();
    assert_eq!(body.statements.len(), 2, "`let n = p.` and `let z = 1;`");
    assert_eq!(
        probe.hole,
        Some(HoleSite::Member {
            name: nazm_span::Span::new(FileId::ROOT, hole, hole)
        })
    );
    assert_eq!(cst.text(), text, "no byte added or lost");
    // One byte off the `.`, in the whitespace after it: no hole, and the ordinary reading.
    let (off, _, errors) = parse_both_at(FileId::ROOT, &text, Some(hole + 1));
    assert_eq!(off.hole, None);
    assert_eq!(errors.len(), 1);
    assert_eq!(off.functions, plain.functions);
}

#[test]
fn a_hole_leaves_every_other_mistake_a_mistake() {
    on_the_parse_stack(a_hole_leaves_every_other_mistake_a_mistake_inner);
}

fn a_hole_leaves_every_other_mistake_a_mistake_inner() {
    // Another error after the hole's statement: still an error, and the body still goes.
    let text =
        format!("{DEFS}fn f(p: Point) -> Int {{\n    let n = p.\n    let bad = ;\n    0\n}}\n");
    let (probe, _, errors) = parse_both_at(FileId::ROOT, &text, Some(at_end_of(&text, "p.")));
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(probe.functions[0].body.is_none());
    // A `;` missing from a later statement is not the hole's to forgive.
    let text = format!(
        "{DEFS}fn f(p: Point) -> Int {{\n    let n = p.\n    let z = 1 let w = 2;\n    w\n}}\n"
    );
    let (probe, _, errors) = parse_both_at(FileId::ROOT, &text, Some(at_end_of(&text, "p.")));
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(probe.functions[0].body.is_none());
    // Two dots: the first is not the hole, and is the error it always was.
    let text = format!("{DEFS}fn f(p: Point) -> Int {{\n    let n = p..\n    0\n}}\n");
    let (probe, _, errors) = parse_both_at(FileId::ROOT, &text, Some(at_end_of(&text, "p..")));
    assert_eq!(probe.hole, None);
    assert_eq!(errors.len(), 1);
    // A label after the `(`: a construction being written, never a hole.
    let text = format!("{DEFS}fn f() -> Point {{\n    Point(x: 1)\n}}\n");
    let (probe, _, errors) = parse_both_at(FileId::ROOT, &text, Some(at_end_of(&text, "Point(")));
    assert_eq!((probe.hole, errors.len()), (None, 0));
    // The holes each shape makes.
    for (body, needle, kind) in [
        ("fn f() -> Int {\n    let p = Point(\n    0\n}\n", "= Point(", "call"),
        ("fn f() -> Int {\n    let p = Point()\n    0\n}\n", "= Point(", "call"),
        ("enum S {\n    A(v: Int),\n}\nfn f() -> Int {\n    let s = S.A(\n    0\n}\n", "S.A(", "payload"),
        ("enum S {\n    A(v: Int),\n}\nfn f(s: S) -> Int {\n    match s {\n        S.A(\n    }\n}\n", "S.A(", "pattern"),
        ("enum M[T] {\n    A(v: T),\n}\nfn f() -> Int {\n    let m = M[Int].\n    0\n}\n", "M[Int].", "member"),
    ] {
        let text = format!("{DEFS}{body}");
        let (probe, _, errors) = parse_both_at(FileId::ROOT, &text, Some(at_end_of(&text, needle)));
        assert_eq!(errors, [], "{body}");
        let got = match probe.hole {
            Some(HoleSite::Call { .. }) => "call",
            Some(HoleSite::Payload { pattern: false, .. }) => "payload",
            Some(HoleSite::Payload { pattern: true, .. }) => "pattern",
            Some(HoleSite::Member { .. }) => "member",
            None => "none",
        };
        assert_eq!(got, kind, "{body}");
    }
}
