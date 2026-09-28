//! The documentation corpus as sections, against controlled fixtures and against the real
//! repository. N32.
//!
//! What is pinned: ids that do not move when a heading is reworded or an anchored section
//! is moved, and do when nothing anchors them; exact bodies — a document's sections
//! concatenate to its bytes; direct bodies that never carry a subsection; every structural
//! refusal; the authority each document keeps; and freshness that tracks the text and
//! nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use nazm_docs::{Authority, Corpus, Document, Format, IndexResult, Kind, SectionResult, CORPUS};

const A: Document = Document {
    id: "a",
    path: "docs/a.md",
    authority: Authority::Normative,
    format: Format::Markdown,
};
const B: Document = Document {
    id: "b",
    path: "docs/b.md",
    authority: Authority::Goal,
    format: Format::Markdown,
};
const G: Document = Document {
    id: "g",
    path: "docs/g.ebnf",
    authority: Authority::Grammar,
    format: Format::Ebnf,
};
const MANIFEST: &[Document] = &[A, B];

fn write(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn fixture(a: &str, b: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", a);
    write(dir.path(), "docs/b.md", b);
    dir
}

fn load(root: &Path, manifest: &[Document]) -> Corpus {
    Corpus::load(root, manifest).unwrap_or_else(|p| panic!("{p:?}"))
}

fn problems(root: &Path, manifest: &[Document]) -> Vec<(String, Option<String>)> {
    Corpus::load(root, manifest)
        .map(|_| Vec::new())
        .unwrap_or_else(|p| {
            p.into_iter()
                .map(|p| (p.kind.to_owned(), p.value))
                .collect()
        })
}

fn section(c: &Corpus, id: &str) -> nazm_docs::DocsSection {
    match c.section(id, None) {
        SectionResult::Available(s) => *s,
        other => panic!("{id}: {other:?}"),
    }
}

fn ids(c: &Corpus) -> Vec<String> {
    let IndexResult::Available(i) = c.index(None) else {
        panic!("no index")
    };
    i.documents
        .iter()
        .flat_map(|d| d.sections.iter().map(|s| s.id.clone()))
        .collect()
}

const DOC_A: &str = "# Title A\n\nPreamble.\n\n## Parent\n<!-- id: parent -->\nparent text\n\n### Child\nchild text\n\n### Other child\nmore\n\n## 7.2 Numbered thing\nnumbered\n\n## Parent two\n### Child\nsame title, other parent\n";
/// A second document with no link into the first, for the tests that rewrite it.
const PLAIN_B: &str =
    "# Goals\n\n## G78 — Documentation Token Efficiency\n\nSelective retrieval.\n";
const DOC_B: &str = "# Goals\n\n## G78 — Documentation Token Efficiency\n\nSelective retrieval. See [the parent](a.md#parent), [a](a.md), [out](../README.md) and [web](https://example.org).\n";

#[test]
fn ids_come_from_anchors_labels_and_heading_paths_in_that_order() {
    let dir = fixture(DOC_A, DOC_B);
    let c = load(dir.path(), MANIFEST);
    assert_eq!(
        ids(&c),
        [
            "a",
            "a:parent",
            "a:parent/child",
            "a:parent/other-child",
            "a:7.2",
            "a:parent-two",
            "a:parent-two/child",
            "b",
            "b:G78"
        ]
    );
    // Two sections titled `Child`, under different parents, are two ids.
    assert_ne!(
        section(&c, "a:parent/child").body,
        section(&c, "a:parent-two/child").body
    );
}

#[test]
fn a_parents_body_is_its_own_text_and_its_children_are_listed() {
    let dir = fixture(DOC_A, DOC_B);
    let c = load(dir.path(), MANIFEST);
    let p = section(&c, "a:parent");
    assert_eq!(p.body, "## Parent\n<!-- id: parent -->\nparent text\n\n");
    assert!(!p.body.contains("child text"));
    assert_eq!(p.children, ["a:parent/child", "a:parent/other-child"]);
    assert_eq!(p.parent.as_deref(), Some("a"));
    assert_eq!(p.kind, Kind::Heading);
    assert_eq!(p.level, 2);
    let root = section(&c, "a");
    assert_eq!(root.body, "# Title A\n\nPreamble.\n\n");
    assert_eq!(root.kind, Kind::Document);
    assert_eq!(root.children, ["a:parent", "a:7.2", "a:parent-two"]);
    // Every section, in order, is the document byte for byte.
    let whole: String = ids(&c)
        .iter()
        .filter(|id| id.starts_with('a'))
        .map(|id| section(&c, id).body)
        .collect();
    assert_eq!(whole, DOC_A);
}

#[test]
fn an_anchored_section_keeps_its_id_when_reworded_or_moved_and_a_path_does_not() {
    let dir = fixture(DOC_A, PLAIN_B);
    let before = load(dir.path(), MANIFEST);
    // Reworded: the anchored section and the labelled one keep their ids; the digest moves.
    let reworded = DOC_A
        .replace("## Parent\n", "## A parent, renamed\n")
        .replace(
            "## 7.2 Numbered thing",
            "## 7.2 A thing, renumbered in words only",
        );
    write(dir.path(), "docs/a.md", &reworded);
    let after = load(dir.path(), MANIFEST);
    for id in ["a:parent", "a:parent/child", "a:7.2"] {
        assert!(ids(&after).contains(&id.to_owned()), "{id}");
    }
    assert_ne!(
        section(&before, "a:parent").digest,
        section(&after, "a:parent").digest
    );
    assert_eq!(
        section(&before, "a:parent/child").digest,
        section(&after, "a:parent/child").digest,
        "an untouched section's digest does not move"
    );
    // Moved under another parent: the same id, a new parent.
    let moved = "# Title A\n\n## Parent two\n\n### Parent\n<!-- id: parent -->\nparent text\n";
    write(dir.path(), "docs/a.md", moved);
    let c = load(dir.path(), MANIFEST);
    assert_eq!(
        section(&c, "a:parent").parent.as_deref(),
        Some("a:parent-two")
    );
    // An unanchored heading reworded is a new id, and its old one is not found.
    let renamed = DOC_A.replace("## Parent two\n", "## Second parent\n");
    write(dir.path(), "docs/a.md", &renamed);
    let c = load(dir.path(), MANIFEST);
    assert!(matches!(
        c.section("a:parent-two", None),
        SectionResult::NotFound(_)
    ));
    assert_eq!(
        section(&c, "a:second-parent/child").title.as_deref(),
        Some("Child")
    );
}

#[test]
fn a_removed_section_is_not_found_and_an_added_one_moves_no_other_id() {
    let dir = fixture(DOC_A, PLAIN_B);
    let before = ids(&load(dir.path(), MANIFEST));
    write(
        dir.path(),
        "docs/a.md",
        &DOC_A.replace("### Other child\nmore\n\n", ""),
    );
    let c = load(dir.path(), MANIFEST);
    assert!(matches!(
        c.section("a:parent/other-child", None),
        SectionResult::NotFound(_)
    ));
    write(
        dir.path(),
        "docs/a.md",
        &DOC_A.replace("## Parent two\n", "## New\nnew\n\n## Parent two\n"),
    );
    let after = ids(&load(dir.path(), MANIFEST));
    let added: Vec<&String> = after.iter().filter(|i| !before.contains(i)).collect();
    assert_eq!(added, ["a:new"]);
    assert!(before.iter().all(|i| after.contains(i)));
}

#[test]
fn a_heading_in_code_is_not_a_section_and_unicode_headings_are_deterministic() {
    let text =
        "# T\n\n```text\n# fake heading\n## also fake\n```\n\n## Ünïcödé — ধারা\n\n### Plain\n";
    let dir = fixture(text, PLAIN_B);
    let c = load(dir.path(), MANIFEST);
    let a: Vec<String> = ids(&c).into_iter().filter(|i| i.starts_with('a')).collect();
    assert_eq!(a.len(), 3, "{a:?}");
    assert!(section(&c, "a").body.contains("# fake heading"));
    let unicode = &a[1];
    assert_eq!(section(&c, unicode).title.as_deref(), Some("Ünïcödé — ধারা"));
    assert_eq!(ids(&load(dir.path(), MANIFEST)), ids(&c));
    // With an ASCII anchor, rewording the Unicode title does not change the id.
    let anchored = text.replace(
        "## Ünïcödé — ধারা\n",
        "## Ünïcödé — ধারা\n<!-- id: unicode -->\n",
    );
    write(dir.path(), "docs/a.md", &anchored);
    let one = load(dir.path(), MANIFEST);
    write(dir.path(), "docs/a.md", &anchored.replace("ধারা", "অন্য"));
    let two = load(dir.path(), MANIFEST);
    assert_eq!(section(&one, "a:unicode").id, section(&two, "a:unicode").id);
    assert_ne!(
        section(&one, "a:unicode").digest,
        section(&two, "a:unicode").digest
    );
}

#[test]
fn every_structural_defect_is_refused_and_named() {
    for (text, kind, value) in [
        ("# T\n## X\n## X\n", "duplicate_id", Some("a:x")),
        (
            "# T\n## X\n<!-- id: one -->\n## Y\n<!-- id: one -->\n",
            "duplicate_id",
            Some("a:one"),
        ),
        ("# T\n## 7.1 A\n## 7.1 B\n", "duplicate_id", Some("a:7.1")),
        (
            "# T\n## X\n<!-- id: Bad Name -->\n",
            "malformed_anchor",
            None,
        ),
        ("# T\n## X\n\n<!-- id: late -->\n", "misplaced_anchor", None),
        ("# T\n\nx\n---\n", "unsupported_heading", None),
        ("# T\n<h2>X</h2>\n", "unsupported_heading", None),
        ("# T\n```\nunclosed\n", "unclosed_fence", None),
        ("no title\n## X\n", "no_title", None),
        ("# T\n## ---\n", "empty_identity", None),
        (
            "# T\n[x](b.md#nowhere)\n",
            "broken_link",
            Some("b.md#nowhere"),
        ),
        ("# T\n[x](#nowhere)\n", "broken_link", Some("#nowhere")),
    ] {
        let dir = fixture(text, DOC_B);
        let p = problems(dir.path(), MANIFEST);
        assert!(
            p.iter()
                .any(|(k, v)| k == kind && (value.is_none() || v.as_deref() == value)),
            "{text:?}: {p:?}"
        );
        // Unavailable is an answer of its own, never a partial index or a stale one.
        assert!(matches!(
            nazm_docs::index_of(dir.path(), MANIFEST, None),
            IndexResult::Unavailable(_)
        ));
        assert!(matches!(
            nazm_docs::section_of(dir.path(), MANIFEST, "a", None),
            SectionResult::Unavailable(_)
        ));
    }
    // A document the manifest names and the disk lacks.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", DOC_A);
    assert_eq!(
        problems(dir.path(), MANIFEST),
        [("missing_document".to_owned(), None)]
    );
}

#[test]
fn links_resolve_to_section_ids_and_outside_links_stay_as_written() {
    let dir = fixture(DOC_A, DOC_B);
    let c = load(dir.path(), MANIFEST);
    let links: Vec<(String, Option<String>, String)> = section(&c, "b:G78")
        .links
        .iter()
        .map(|l| {
            (
                l.href.clone().unwrap(),
                l.target.clone(),
                l.scope.to_owned(),
            )
        })
        .collect();
    assert_eq!(
        links,
        [
            (
                "a.md#parent".to_owned(),
                Some("a:parent".to_owned()),
                "corpus".to_owned()
            ),
            ("a.md".to_owned(), Some("a".to_owned()), "corpus".to_owned()),
            ("../README.md".to_owned(), None, "outside".to_owned()),
            (
                "https://example.org".to_owned(),
                None,
                "external".to_owned()
            ),
        ]
    );
}

#[test]
fn a_link_to_a_reworded_heading_is_broken_and_refused() {
    // A `#fragment` names the heading's rendered anchor, as a reader's browser resolves it:
    // rewording the heading breaks the link, and the corpus says so rather than guessing.
    let dir = fixture(&DOC_A.replace("## Parent\n", "## Renamed\n"), DOC_B);
    assert_eq!(
        problems(dir.path(), MANIFEST),
        [("broken_link".to_owned(), Some("a.md#parent".to_owned()))]
    );
}

#[test]
fn a_goal_is_a_goal_and_the_normative_document_is_normative() {
    let dir = fixture(DOC_A, DOC_B);
    let c = load(dir.path(), MANIFEST);
    assert_eq!(section(&c, "b:G78").authority, Authority::Goal);
    assert_eq!(
        section(&c, "a:parent/child").authority,
        Authority::Normative
    );
}

#[test]
fn a_grammar_is_one_section_per_production_with_exact_text() {
    let grammar = "(* header *)\n\n(* prose *)\n(* @example x *)\na = b , \"x\" ;\n\nb = \"y\"\n  | \";\" ;\n";
    let dir = fixture(DOC_A, DOC_B);
    write(dir.path(), "docs/g.ebnf", grammar);
    let c = load(dir.path(), &[A, G]);
    let a = section(&c, "g:a");
    assert_eq!(a.kind, Kind::Production);
    assert_eq!(a.authority, Authority::Grammar);
    assert_eq!(a.body, "(* prose *)\n(* @example x *)\na = b , \"x\" ;\n");
    let p = a.production.unwrap();
    assert_eq!(p.text, "a = b , \"x\" ;");
    assert_eq!(p.examples, ["x"]);
    assert_eq!(a.links[0].relation, "references");
    assert_eq!(a.links[0].target.as_deref(), Some("g:b"));
    assert_eq!(
        section(&c, "g:b").production.unwrap().text,
        "b = \"y\"\n  | \";\" ;"
    );
    assert_eq!(section(&c, "g").body, "(* header *)\n\n");
    for bad in ["a = b ;\n", "a = \"x\"\n", "a = \"x\" ;\na = \"y\" ;\n"] {
        write(dir.path(), "docs/g.ebnf", bad);
        assert!(Corpus::load(dir.path(), &[A, G]).is_err(), "{bad}");
    }
}

#[test]
fn freshness_tracks_the_text_and_a_stale_state_is_refused() {
    let dir = fixture(DOC_A, DOC_B);
    let c = load(dir.path(), MANIFEST);
    let state = c.state().to_owned();
    assert!(matches!(
        c.section("a:parent", Some(&state)),
        SectionResult::Available(_)
    ));
    // The same text is the same state; one byte of it is another.
    assert_eq!(load(dir.path(), MANIFEST).state(), state);
    write(dir.path(), "docs/a.md", &DOC_A.replace("more", "more."));
    let edited = load(dir.path(), MANIFEST);
    assert_ne!(edited.state(), state);
    match edited.section("a:parent", Some(&state)) {
        SectionResult::Stale(r) => assert_eq!(r.state, edited.state()),
        other => panic!("{other:?}"),
    }
    // Only the edited section's digest moved.
    for id in ids(&c) {
        let changed = section(&c, &id).digest != section(&edited, &id).digest;
        assert_eq!(changed, id == "a:parent/other-child", "{id}");
    }
    // A document added to or removed from the manifest is another corpus.
    assert_ne!(
        load(dir.path(), &[A]).state(),
        load(dir.path(), MANIFEST).state()
    );
    // An unknown id is not found — never the nearest one, and never a path.
    for id in [
        "a:paren",
        "A:parent",
        "../README.md",
        "/etc/passwd",
        "docs/a.md",
        "",
    ] {
        assert!(
            matches!(edited.section(id, None), SectionResult::NotFound(_)),
            "{id}"
        );
    }
}

#[test]
fn the_state_is_the_same_from_any_directory_and_names_none() {
    let one = fixture(DOC_A, DOC_B);
    let two = fixture(DOC_A, DOC_B);
    let (a, b) = (load(one.path(), MANIFEST), load(two.path(), MANIFEST));
    assert_eq!(a.state(), b.state());
    assert_eq!(a.index(None).to_json(), b.index(None).to_json());
    assert_eq!(
        a.section("a:parent", None).to_json(),
        b.section("a:parent", None).to_json()
    );
    assert!(!a
        .index(None)
        .to_json()
        .contains(one.path().to_str().unwrap()));
}

// --- The real corpus. ---

fn real() -> Corpus {
    load(&nazm_docs::default_root(), CORPUS)
}

#[test]
fn the_real_corpus_is_sectioned_and_every_document_keeps_its_authority() {
    let c = real();
    let IndexResult::Available(i) = c.index(None) else {
        panic!()
    };
    let authority = |id: &str| i.documents.iter().find(|d| d.id == id).unwrap().authority;
    assert_eq!(authority("spec"), Authority::Normative);
    assert_eq!(authority("grammar"), Authority::Grammar);
    assert_eq!(authority("architecture"), Authority::Architecture);
    assert_eq!(authority("roadmap"), Authority::Planning);
    assert_eq!(authority("goals"), Authority::Goal);
    assert_eq!(authority("capabilities"), Authority::Evidence);
    assert_eq!(i.documents.len(), CORPUS.len());
    // A goal is never evidence: every goal section says so.
    assert_eq!(section(&c, "goals:G78").authority, Authority::Goal);
    assert!(section(&c, "goals:G78")
        .body
        .starts_with("## G78 — Documentation Token Efficiency\n"));
    assert_eq!(section(&c, "roadmap:N31").authority, Authority::Planning);
}

#[test]
fn every_real_markdown_document_is_its_sections_byte_for_byte() {
    let c = real();
    let root = nazm_docs::default_root();
    let IndexResult::Available(i) = c.index(None) else {
        panic!()
    };
    for d in i.documents.iter().filter(|d| d.format == Format::Markdown) {
        let whole: String = d.sections.iter().map(|s| section(&c, &s.id).body).collect();
        assert_eq!(
            whole,
            std::fs::read_to_string(root.join(d.path)).unwrap(),
            "{}",
            d.id
        );
    }
    // A production is exactly its lines of the grammar.
    let grammar = std::fs::read_to_string(root.join("docs/grammar.ebnf")).unwrap();
    let f = section(&c, "grammar:function");
    assert!(grammar.contains(&f.body));
    assert!(f
        .production
        .unwrap()
        .text
        .starts_with("function = [ visibility ] , \"fn\""));
}

#[test]
fn the_real_spec_is_addressed_by_its_anchors_and_its_bodies_are_direct() {
    let c = real();
    let g = section(&c, "spec:generics");
    assert!(g
        .body
        .starts_with("### Generics — first-order parametric types, and `Vec[T]`\n"));
    assert!(
        !g.body.contains("\n#### "),
        "a parent's body carries no subsection"
    );
    assert!(g.children.contains(&"spec:generics/identity".to_owned()));
    assert_eq!(
        section(&c, "spec:generics/identity").parent.as_deref(),
        Some("spec:generics")
    );
    // The one fragment link in the corpus resolves to the anchored section it names.
    let returns = section(&c, "spec:returns");
    assert!(returns.links.iter().any(
        |l| l.href.as_deref() == Some("#return") && l.target.as_deref() == Some("spec:return")
    ));
}

#[test]
fn every_generated_guide_rule_names_the_production_it_is_generated_from() {
    let c = real();
    let IndexResult::Available(i) = c.index(None) else {
        panic!()
    };
    let productions: Vec<String> = i
        .documents
        .iter()
        .find(|d| d.id == "grammar")
        .unwrap()
        .sections
        .iter()
        .filter(|s| s.kind == Kind::Production)
        .map(|s| s.id.clone())
        .collect();
    let generated: Vec<String> = i
        .documents
        .iter()
        .find(|d| d.id == "guide")
        .unwrap()
        .sections
        .iter()
        .flat_map(|s| section(&c, &s.id).links)
        .filter(|l| l.relation == "generated_from")
        .filter_map(|l| l.target)
        .collect();
    assert_eq!(generated, productions);
}
