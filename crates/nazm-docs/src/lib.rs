//! Nazm's own documentation as stable, authority-classified sections, each retrieved by
//! exact id with its exact canonical text. N32.
//!
//! # The contract
//!
//! **The index is not a second source of documentation truth.** The documents under
//! `docs/` are the authorities; this crate reads them, as they are on disk at each call,
//! into sections, and answers with their text byte for byte. It never summarises,
//! paraphrases or rewrites a rule, and it holds no copy of any document's text: the
//! manifest ([`CORPUS`]) is ids, paths, authorities and readers, nothing else.
//!
//! **Goals, roadmaps, architecture, evidence and normative specification keep distinct
//! authority classes.** Every document, and so every section, carries its [`Authority`]:
//! a goal is never evidence of what exists, and a plan is never a rule of the language.
//!
//! # Identity
//!
//! A section's id is `document:local`, and a document's root section — its title and the
//! text before its first subheading — is `document` alone. The local part is, in order of
//! preference:
//!
//! 1. an **explicit anchor**, `<!-- id: name -->` on the line directly below the heading:
//!    stable across a heading's rewording and a section's move;
//! 2. the heading's **own label**, where it has one — `7.33`, `2a`, `G78`, `N31`, `R1` — as
//!    numbered documents already number their sections: stable across rewording;
//! 3. otherwise the heading's text as a segment, `lower-case-words`, under its parent's
//!    local id: `generics/identity`.
//!
//! Never a line number, a byte offset, a position among siblings, a hash, a random value or
//! a Git commit. Two sections with one id are refused, never numbered apart. A grammar
//! production is `grammar:name`.
//!
//! # Bodies
//!
//! A section's body is its heading line through the byte before the next heading of any
//! level — its **direct** text, never its subsections', which are listed as children.
//! Exact bytes of the file: an explicit anchor line is part of it. A production's body is
//! its attached comments and the production through `;`.
//!
//! # Freshness
//!
//! The corpus `state` is BLAKE3 over a domain tag and, in corpus order, each document's id,
//! its path from the repository root and its exact bytes — no absolute path, time or
//! commit. A section's digest is BLAKE3 over another tag, its document, its id and its
//! body. A section asked for with a `state` that is not the current one is `stale_state`,
//! never an answer from another corpus.

mod grammar;
pub mod manifest;
mod markdown;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

pub use manifest::{Authority, Document, Format, CORPUS};

/// The schema every index declares.
pub const INDEX_SCHEMA: &str = "nazm.docs-index/1";
/// The schema every section declares.
pub const SECTION_SCHEMA: &str = "nazm.docs-section/1";

/// The markers around the guide's part generated from the grammar, as `xtask`'s generator
/// writes them. A guide section between them titled `` `name` `` is generated from
/// `grammar:name`.
pub const GUIDE_BEGIN: &str = "<!-- generated from docs/grammar.ebnf: begin -->";
/// The end marker.
pub const GUIDE_END: &str = "<!-- generated from docs/grammar.ebnf: end -->";

const CORPUS_TAG: &[u8] = b"nazm.docs-corpus/1";
const DOCUMENT_TAG: &[u8] = b"nazm.docs-document/1";
const SECTION_TAG: &[u8] = b"nazm.docs-section/1";

/// The repository this binary was built from: where Nazm's own documentation is when no
/// other root is named. Used to find files, never written into an answer.
#[must_use]
pub fn default_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| crate_dir.to_path_buf(), Path::to_path_buf)
}

/// Why a corpus has no index: each problem, by kind, document and line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub kind: &'static str,
    pub document: String,
    /// From one, where the problem has a line.
    pub line: Option<usize>,
    /// The offending anchor, id, link or name, where there is one.
    pub value: Option<String>,
}

impl Problem {
    pub(crate) fn at(kind: &'static str, document: &str, line: usize, value: &str) -> Self {
        Self {
            kind,
            document: document.to_owned(),
            line: Some(line + 1),
            value: (!value.is_empty()).then(|| value.to_owned()),
        }
    }

    pub(crate) fn of(kind: &'static str, document: &str) -> Self {
        Self {
            kind,
            document: document.to_owned(),
            line: None,
            value: None,
        }
    }
}

/// What a section is structurally. Nothing here classifies content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A document's title and the text before its first subheading.
    Document,
    /// A Markdown heading and its direct text.
    Heading,
    /// A grammar production and its attached comments.
    Production,
}

/// An explicit reference from a section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// `link` — written in the text; `generated_from` — a guide section the grammar
    /// generates; `references` — a nonterminal a production names.
    pub relation: &'static str,
    /// As written, for a `link`.
    pub href: Option<String>,
    /// The section it resolves to, for one inside the corpus.
    pub target: Option<String>,
    /// `corpus`, `outside` (a repository path the corpus does not include) or `external`.
    pub scope: &'static str,
}

/// A production's structure, beside its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Production {
    /// `name = … ;` exactly.
    pub text: String,
    /// Each attached `@example`, exactly.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone)]
struct Section {
    id: String,
    kind: Kind,
    title: Option<String>,
    level: u8,
    parent: Option<String>,
    children: Vec<String>,
    range: (usize, usize),
    lines: (usize, usize),
    digest: String,
    links: Vec<Link>,
    production: Option<Production>,
    /// The fragment a `#…` link names it by, where it is a heading.
    fragment: Option<String>,
}

#[derive(Debug, Clone)]
struct Loaded {
    spec: Document,
    text: String,
    digest: String,
    sections: Vec<Section>,
}

/// A whole corpus, read and sectioned. Request-local: nothing outlives the call that built
/// it.
#[derive(Debug, Clone)]
pub struct Corpus {
    state: String,
    documents: Vec<Loaded>,
}

impl Corpus {
    /// Read `manifest`'s documents under `root` and section them, or every problem that
    /// makes the corpus unindexable. Only the manifest's paths are opened.
    ///
    /// # Errors
    ///
    /// A missing or non-UTF-8 document, a malformed structure, a duplicate id, a broken
    /// in-corpus link, or a grammar that does not parse.
    pub fn load(root: &Path, manifest: &[Document]) -> Result<Self, Vec<Problem>> {
        let mut problems = Vec::new();
        let mut texts = Vec::new();
        for spec in manifest {
            match std::fs::read(root.join(spec.path)) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(t) => texts.push((*spec, t)),
                    Err(_) => problems.push(Problem::of("unreadable_document", spec.id)),
                },
                Err(_) => problems.push(Problem::of("missing_document", spec.id)),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }

        let mut state = blake3::Hasher::new();
        framed(&mut state, CORPUS_TAG);
        let mut documents = Vec::new();
        for (spec, text) in texts {
            for part in [spec.id.as_bytes(), spec.path.as_bytes(), text.as_bytes()] {
                framed(&mut state, part);
            }
            let mut d = blake3::Hasher::new();
            for part in [
                DOCUMENT_TAG,
                spec.id.as_bytes(),
                spec.path.as_bytes(),
                text.as_bytes(),
            ] {
                framed(&mut d, part);
            }
            let sectioned = match spec.format {
                Format::Markdown => markdown_sections(&spec, &text),
                Format::Ebnf => grammar_sections(&spec, &text),
            };
            match sectioned {
                Ok(sections) => documents.push(Loaded {
                    spec,
                    digest: d.finalize().to_hex().to_string(),
                    text,
                    sections,
                }),
                Err(mut p) => problems.append(&mut p),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        resolve(&mut documents, &mut problems);
        if problems.is_empty() {
            Ok(Self {
                state: state.finalize().to_hex().to_string(),
                documents,
            })
        } else {
            Err(problems)
        }
    }

    /// The corpus state.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Documents, sections and bytes.
    #[must_use]
    pub fn counts(&self) -> Counts {
        Counts {
            documents: self.documents.len(),
            sections: self.documents.iter().map(|d| d.sections.len()).sum(),
            bytes: self.documents.iter().map(|d| d.text.len()).sum(),
        }
    }

    /// The compact index, of every document or of one.
    #[must_use]
    pub fn index(&self, document: Option<&str>) -> IndexResult {
        if let Some(wanted) = document {
            if !self.documents.iter().any(|d| d.spec.id == wanted) {
                return IndexResult::NotFound(IndexNotFound {
                    schema: INDEX_SCHEMA,
                    status: "not_found",
                    state: self.state.clone(),
                    document: wanted.to_owned(),
                });
            }
        }
        IndexResult::Available(Index {
            schema: INDEX_SCHEMA,
            status: "available",
            state: self.state.clone(),
            corpus: self.counts(),
            documents: self
                .documents
                .iter()
                .filter(|d| document.is_none_or(|w| d.spec.id == w))
                .map(|d| IndexDocument {
                    id: d.spec.id,
                    path: d.spec.path,
                    authority: d.spec.authority,
                    format: d.spec.format,
                    bytes: d.text.len(),
                    digest: d.digest.clone(),
                    sections: d
                        .sections
                        .iter()
                        .map(|s| IndexSection {
                            id: s.id.clone(),
                            kind: s.kind,
                            title: s.title.clone(),
                            level: s.level,
                            parent: s.parent.clone(),
                            digest: s.digest.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    /// One section by its exact id, if `state` — when given — is this corpus's.
    #[must_use]
    pub fn section(&self, id: &str, state: Option<&str>) -> SectionResult {
        if state.is_some_and(|s| s != self.state) {
            return SectionResult::Stale(SectionRefused {
                schema: SECTION_SCHEMA,
                status: "stale_state",
                state: self.state.clone(),
                id: id.to_owned(),
            });
        }
        let found = self
            .documents
            .iter()
            .find_map(|d| d.sections.iter().find(|s| s.id == id).map(|s| (d, s)));
        let Some((d, s)) = found else {
            return SectionResult::NotFound(SectionRefused {
                schema: SECTION_SCHEMA,
                status: "not_found",
                state: self.state.clone(),
                id: id.to_owned(),
            });
        };
        SectionResult::Available(Box::new(DocsSection {
            schema: SECTION_SCHEMA,
            status: "available",
            state: self.state.clone(),
            id: s.id.clone(),
            document: d.spec.id,
            authority: d.spec.authority,
            format: d.spec.format,
            kind: s.kind,
            title: s.title.clone(),
            level: s.level,
            parent: s.parent.clone(),
            children: s.children.clone(),
            source: Source {
                path: d.spec.path,
                lines: [s.lines.0, s.lines.1],
            },
            body: d
                .text
                .get(s.range.0..s.range.1)
                .unwrap_or_default()
                .to_owned(),
            digest: s.digest.clone(),
            links: s.links.clone(),
            production: s.production.clone(),
        }))
    }
}

/// The corpus's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub documents: usize,
    pub sections: usize,
    pub bytes: usize,
}

/// `nazm.docs-index/1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Index {
    pub schema: &'static str,
    pub status: &'static str,
    pub state: String,
    pub corpus: Counts,
    pub documents: Vec<IndexDocument>,
}

/// One document in an index: its authority, and its sections' metadata — no body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexDocument {
    pub id: &'static str,
    pub path: &'static str,
    pub authority: Authority,
    pub format: Format,
    pub bytes: usize,
    pub digest: String,
    /// In source order.
    pub sections: Vec<IndexSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexSection {
    pub id: String,
    pub kind: Kind,
    pub title: Option<String>,
    pub level: u8,
    pub parent: Option<String>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexNotFound {
    pub schema: &'static str,
    pub status: &'static str,
    pub state: String,
    pub document: String,
}

/// A corpus with no index, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    pub schema: &'static str,
    pub status: &'static str,
    pub problems: Vec<Problem>,
}

/// What an index request answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IndexResult {
    Available(Index),
    NotFound(IndexNotFound),
    Unavailable(Unavailable),
}

/// `nazm.docs-section/1`: one section, its authority and its exact text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocsSection {
    pub schema: &'static str,
    pub status: &'static str,
    pub state: String,
    pub id: String,
    pub document: &'static str,
    pub authority: Authority,
    pub format: Format,
    pub kind: Kind,
    pub title: Option<String>,
    pub level: u8,
    pub parent: Option<String>,
    /// Its direct subsections, in source order.
    pub children: Vec<String>,
    pub source: Source,
    /// The canonical text, byte for byte.
    pub body: String,
    pub digest: String,
    pub links: Vec<Link>,
    /// For a grammar production.
    pub production: Option<Production>,
}

/// Where a section is: its document's path and its first and last lines, from one.
/// Location, never identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    pub path: &'static str,
    pub lines: [usize; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SectionRefused {
    pub schema: &'static str,
    pub status: &'static str,
    pub state: String,
    pub id: String,
}

/// What a section request answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SectionResult {
    Available(Box<DocsSection>),
    NotFound(SectionRefused),
    Stale(SectionRefused),
    Unavailable(Unavailable),
}

/// The one canonical serialisation of each answer.
macro_rules! json {
    ($t:ty) => {
        impl $t {
            /// Compact JSON, one line.
            #[must_use]
            pub fn to_json(&self) -> String {
                serde_json::to_string(self).unwrap_or_default()
            }
        }
    };
}
json!(IndexResult);
json!(SectionResult);

/// The index of Nazm's corpus under `root`, of every document or of one.
#[must_use]
pub fn index(root: &Path, document: Option<&str>) -> IndexResult {
    index_of(root, CORPUS, document)
}

/// The index of `manifest`'s corpus under `root`.
#[must_use]
pub fn index_of(root: &Path, manifest: &[Document], document: Option<&str>) -> IndexResult {
    match Corpus::load(root, manifest) {
        Ok(c) => c.index(document),
        Err(problems) => IndexResult::Unavailable(Unavailable {
            schema: INDEX_SCHEMA,
            status: "unavailable",
            problems,
        }),
    }
}

/// One section of Nazm's corpus under `root`, by exact id, bound to `state` when given.
#[must_use]
pub fn section(root: &Path, id: &str, state: Option<&str>) -> SectionResult {
    section_of(root, CORPUS, id, state)
}

/// One section of `manifest`'s corpus under `root`.
#[must_use]
pub fn section_of(
    root: &Path,
    manifest: &[Document],
    id: &str,
    state: Option<&str>,
) -> SectionResult {
    match Corpus::load(root, manifest) {
        Ok(c) => c.section(id, state),
        Err(problems) => SectionResult::Unavailable(Unavailable {
            schema: SECTION_SCHEMA,
            status: "unavailable",
            problems,
        }),
    }
}

/// Length-prefixed, so no two sequences of parts frame to the same bytes.
fn framed(h: &mut blake3::Hasher, part: &[u8]) {
    h.update(&(part.len() as u64).to_le_bytes());
    h.update(part);
}

fn digest(document: &str, id: &str, body: &str) -> String {
    let mut h = blake3::Hasher::new();
    for part in [
        SECTION_TAG,
        document.as_bytes(),
        id.as_bytes(),
        body.as_bytes(),
    ] {
        framed(&mut h, part);
    }
    h.finalize().to_hex().to_string()
}

/// Lines `range` covers, from one: its first line and its last.
fn lines_of(text: &str, range: (usize, usize)) -> (usize, usize) {
    let before = text.get(..range.0).unwrap_or_default();
    let first = before.matches('\n').count() + 1;
    let inside = text.get(range.0..range.1).unwrap_or_default();
    let last = first + inside.trim_end_matches('\n').matches('\n').count();
    (first, last.max(first))
}

/// A heading's own label: `7.33`, `2a`, `6A`, `11.1` — digits, an optional letter, dotted
/// digits, then an optional `.` and a space — or `G78`, `N14.1`, `R1` — one capital, digits,
/// dotted digits, then a space and a dash or a middle dot.
fn label(title: &str) -> Option<String> {
    let numeric = || {
        let digits = title.chars().take_while(char::is_ascii_digit).count();
        (digits > 0).then_some(())?;
        let mut end = digits;
        if title
            .get(end..)?
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            end += 1;
        }
        end = dotted(title, end)?;
        let rest = title.get(end..)?;
        let rest = rest.strip_prefix('.').unwrap_or(rest);
        rest.starts_with(' ')
            .then(|| title.get(..end).map(str::to_owned))?
    };
    let lettered = || {
        let first = title.chars().next().filter(char::is_ascii_uppercase)?;
        let digits = title
            .get(1..)?
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        (digits > 0).then_some(())?;
        let end = dotted(title, first.len_utf8() + digits)?;
        let rest = title.get(end..)?.strip_prefix(' ')?;
        (rest.starts_with('—') || rest.starts_with('·') || rest.starts_with("- "))
            .then(|| title.get(..end).map(str::to_owned))?
    };
    numeric().or_else(lettered)
}

/// Past any `.digits` groups from `at`.
fn dotted(title: &str, mut at: usize) -> Option<usize> {
    loop {
        let rest = title.get(at..)?;
        let Some(after) = rest.strip_prefix('.') else {
            return Some(at);
        };
        let n = after.chars().take_while(char::is_ascii_digit).count();
        if n == 0 {
            return Some(at);
        }
        at += 1 + n;
    }
}

fn markdown_sections(spec: &Document, text: &str) -> Result<Vec<Section>, Vec<Problem>> {
    let headings = markdown::headings(spec.id, text)?;
    let mut problems = Vec::new();
    let Some(title) = headings.first().filter(|h| h.level == 1 && h.start == 0) else {
        return Err(vec![Problem::of("no_title", spec.id)]);
    };
    if title.anchor.is_some() {
        problems.push(Problem::at("misplaced_anchor", spec.id, title.line, ""));
    }
    let end_of = |i: usize| headings.get(i + 1).map_or(text.len(), |h| h.start);
    // From one: a section's first line is its heading's, its last the line before the next
    // heading's, or the document's last.
    let last_line = text.trim_end_matches('\n').matches('\n').count() + 1;
    let lines_at = |i: usize| {
        let first = headings.get(i).map_or(1, |h| h.line + 1);
        let last = headings.get(i + 1).map_or(last_line, |h| h.line);
        (first, last.max(first))
    };

    let mut sections = vec![Section {
        id: spec.id.to_owned(),
        kind: Kind::Document,
        title: Some(title.title.clone()),
        level: 1,
        parent: None,
        children: Vec::new(),
        range: (0, end_of(0)),
        lines: lines_at(0),
        digest: String::new(),
        links: Vec::new(),
        production: None,
        fragment: Some(markdown::github_slug(&title.title)),
    }];
    // Open ancestors: level, local id, index in `sections`.
    let mut stack: Vec<(u8, String, usize)> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, h) in headings.iter().enumerate().skip(1) {
        while stack.last().is_some_and(|(level, _, _)| *level >= h.level) {
            stack.pop();
        }
        let local = if let Some(anchor) = &h.anchor {
            anchor.clone()
        } else if let Some(label) = label(&h.title) {
            label
        } else {
            let segment = markdown::slug(&h.title);
            if segment.is_empty() {
                problems.push(Problem::at("empty_identity", spec.id, h.line, &h.title));
                continue;
            }
            match stack.last() {
                Some((_, parent, _)) => format!("{parent}/{segment}"),
                None => segment,
            }
        };
        let id = format!("{}:{local}", spec.id);
        if seen.insert(id.clone(), h.line).is_some() {
            problems.push(Problem::at("duplicate_id", spec.id, h.line, &id));
            continue;
        }
        let parent_index = stack.last().map_or(0, |(_, _, p)| *p);
        let parent = sections.get(parent_index).map(|p| p.id.clone());
        let me = sections.len();
        if let Some(p) = sections.get_mut(parent_index) {
            p.children.push(id.clone());
        }
        sections.push(Section {
            id,
            kind: Kind::Heading,
            title: Some(h.title.clone()),
            level: h.level,
            parent,
            children: Vec::new(),
            range: (h.start, end_of(i)),
            lines: lines_at(i),
            digest: String::new(),
            links: Vec::new(),
            production: None,
            fragment: Some(markdown::github_slug(&h.title)),
        });
        stack.push((h.level, local, me));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    // GitHub numbers a repeated anchor `-1`, `-2`, … in document order.
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for s in &mut sections {
        if let Some(f) = s.fragment.take() {
            let n = used.entry(f.clone()).or_insert(0);
            s.fragment = Some(if *n == 0 {
                f.clone()
            } else {
                format!("{f}-{n}")
            });
            *n += 1;
        }
        let body = text.get(s.range.0..s.range.1).unwrap_or_default();
        s.digest = digest(spec.id, &s.id, body);
        s.links = markdown::links(body)
            .into_iter()
            .map(|href| Link {
                relation: "link",
                href: Some(href),
                target: None,
                scope: "outside",
            })
            .collect();
    }
    Ok(sections)
}

fn grammar_sections(spec: &Document, text: &str) -> Result<Vec<Section>, Vec<Problem>> {
    let (header, rules) = grammar::rules(spec.id, text)?;
    let mut root = Section {
        id: spec.id.to_owned(),
        kind: Kind::Document,
        title: None,
        level: 1,
        parent: None,
        children: Vec::new(),
        range: (0, header),
        lines: lines_of(text, (0, header)),
        digest: digest(spec.id, spec.id, text.get(..header).unwrap_or_default()),
        links: Vec::new(),
        production: None,
        fragment: None,
    };
    let mut sections = Vec::new();
    for r in rules {
        let id = format!("{}:{}", spec.id, r.name);
        root.children.push(id.clone());
        let body = text.get(r.block.0..r.block.1).unwrap_or_default();
        sections.push(Section {
            digest: digest(spec.id, &id, body),
            kind: Kind::Production,
            title: Some(r.name.clone()),
            level: 2,
            parent: Some(spec.id.to_owned()),
            children: Vec::new(),
            range: r.block,
            lines: lines_of(text, r.block),
            links: r
                .references
                .iter()
                .map(|n| Link {
                    relation: "references",
                    href: None,
                    target: Some(format!("{}:{n}", spec.id)),
                    scope: "corpus",
                })
                .collect(),
            production: Some(Production {
                text: text
                    .get(r.production.0..r.production.1)
                    .unwrap_or_default()
                    .to_owned(),
                examples: r.examples,
            }),
            fragment: None,
            id,
        });
    }
    sections.insert(0, root);
    Ok(sections)
}

/// Resolve every written link against the corpus, and attach each generated guide section
/// to the production it is generated from. A link into the corpus that names no section is
/// a problem; one outside it is kept as written.
fn resolve(documents: &mut [Loaded], problems: &mut Vec<Problem>) {
    let paths: Vec<(&'static str, &'static str)> =
        documents.iter().map(|d| (d.spec.path, d.spec.id)).collect();
    let fragments: BTreeMap<(&'static str, String), String> = documents
        .iter()
        .flat_map(|d| {
            d.sections
                .iter()
                .filter_map(move |s| s.fragment.clone().map(|f| ((d.spec.id, f), s.id.clone())))
        })
        .collect();
    let ids: std::collections::BTreeSet<String> = documents
        .iter()
        .flat_map(|d| d.sections.iter().map(|s| s.id.clone()))
        .collect();
    let grammar = documents
        .iter()
        .find(|d| d.spec.format == Format::Ebnf)
        .map(|d| d.spec.id);

    for d in documents.iter_mut() {
        let generated = if d.spec.authority == Authority::Guide {
            match (d.text.find(GUIDE_BEGIN), d.text.find(GUIDE_END), grammar) {
                (Some(b), Some(e), Some(g))
                    if b < e
                        && d.text.matches(GUIDE_BEGIN).count() == 1
                        && d.text.matches(GUIDE_END).count() == 1 =>
                {
                    Some((b, e, g))
                }
                _ => {
                    problems.push(Problem::of("generated_region", d.spec.id));
                    None
                }
            }
        } else {
            None
        };
        let base = Path::new(d.spec.path).parent().unwrap_or(Path::new(""));
        for s in &mut d.sections {
            for link in &mut s.links {
                let Some(href) = link.href.clone() else {
                    continue;
                };
                if href.contains("://") || href.starts_with("mailto:") {
                    link.scope = "external";
                    continue;
                }
                let (path, fragment) = match href.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (href.as_str(), None),
                };
                let target_doc = if path.is_empty() {
                    Some(d.spec.id)
                } else {
                    normal(&base.join(path))
                        .and_then(|p| paths.iter().find(|(q, _)| *q == p).map(|(_, id)| *id))
                };
                let Some(target_doc) = target_doc else {
                    continue;
                };
                let target = match fragment {
                    None => Some(target_doc.to_owned()),
                    Some(f) => fragments.get(&(target_doc, f.to_owned())).cloned(),
                };
                match target {
                    Some(t) => {
                        link.target = Some(t);
                        link.scope = "corpus";
                    }
                    None => {
                        problems.push(Problem::at("broken_link", d.spec.id, s.lines.0 - 1, &href))
                    }
                }
            }
            if let (Some((b, e, g)), Some(title)) = (generated, &s.title) {
                let name = title.strip_prefix('`').and_then(|t| t.strip_suffix('`'));
                if let (true, Some(name)) = (s.range.0 > b && s.range.0 < e, name) {
                    let target = format!("{g}:{name}");
                    if ids.contains(&target) {
                        s.links.push(Link {
                            relation: "generated_from",
                            href: None,
                            target: Some(target),
                            scope: "corpus",
                        });
                    } else {
                        problems.push(Problem::at(
                            "broken_generated_from",
                            d.spec.id,
                            s.lines.0 - 1,
                            &target,
                        ));
                    }
                }
            }
        }
    }
}

/// `path` without `.` and `..`, with `/`; `None` if it climbs out of the repository.
fn normal(path: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str()?.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::label;

    #[test]
    fn a_label_is_the_documents_own_numbering_and_nothing_else() {
        for (title, want) in [
            ("7.33 Token-efficient command", Some("7.33")),
            ("7. Crate layout", Some("7")),
            ("2a. Incremental semantic checking — VERIFIED", Some("2a")),
            ("6A. Best-in-Class", Some("6A")),
            ("11.1 Value", Some("11.1")),
            ("G78 — Documentation Token Efficiency", Some("G78")),
            ("N14.1 — Contained performance calibration", Some("N14.1")),
            ("R1 · Mutable value semantics", Some("R1")),
            ("After N31, in dependency order", None),
            ("Stage A — Correct language core", None),
            ("`return`", None),
            ("1.1What", None),
            ("G — nothing", None),
        ] {
            assert_eq!(label(title).as_deref(), want, "{title}");
        }
    }
}
