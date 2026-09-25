//! The language service: compiler answers for the files an editor has open. N16.
//!
//! # What it is, and what it is not
//!
//! A long-lived owner of **open documents** — their exact current text and version — that
//! answers three questions about them: what is wrong ([`Published`] diagnostics), where a
//! name is defined ([`LanguageService::definition`]), and what a name is
//! ([`LanguageService::hover`]). It speaks no protocol: positions are byte offsets into a
//! document's text and every answer names files by path. `nazm lsp` is an adapter over it.
//!
//! **It is not a second compiler.** It has no lexer, parser, symbol table, resolver or
//! checker of its own. Every answer comes from three things the compiler already owns:
//!
//! - [`crate::load::load_from`] — the loader every command uses, reading an open buffer in
//!   place of the file on disk;
//! - [`nazm_core::analyse`] — the same parse and check every command runs;
//! - [`nazm_core::Resolution`] — what that check decided each name refers to, by identity.
//!
//! The concrete tree ([`nazm_core::__syntax::Cst`]) finds *which token* a position is on. What that
//! token *means* is only ever read from the resolution. A query that the resolution cannot
//! answer — a name in a body that did not parse, a type name it does not record — has no
//! answer; nothing here guesses from spelling.
//!
//! # What is recomputed
//!
//! Everything, on every change, for every open document: each is loaded as the root of a
//! compilation — exactly the compilation `nazm check <that file>` would load — lexed and
//! parsed in full, and checked. That is the simplest strategy that is correct, and it is
//! the one taken: no incremental reparse, no graph maintenance, no reuse of semantic work
//! between versions, and no persistent cache read or written (an unsaved buffer is not
//! state the cache may record). What *is* reused is the process, and the open buffers.
//! `docs/performance.md` measures what that costs.
//!
//! # Current means current
//!
//! Each analysis records the **generation** it was computed at — a counter bumped by every
//! open, change and close — and the document version it saw. A query is answered only from
//! an analysis whose generation is the service's current one and whose version is the
//! version the caller asked about; otherwise it has no answer. Because a mutation
//! recomputes every open document before returning, a stale analysis cannot exist today —
//! the check is there so that correctness does not depend on that.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nazm_core::__syntax::{Cst, SyntaxKind};
use nazm_core::sema::module::ModuleGraph;
use nazm_core::sema::{Callee, FnDef};
use nazm_core::{Analysis, Entry, Resolution};
use nazm_diag::Diagnostic;
use nazm_span::{FileId, SourceMap, Span};

use crate::load::{self, Disk, Sources};
use crate::root;

/// Where something is: a file, by path, and a byte range in that file's analysed text.
///
/// The text travels with it because a protocol position is a function of the text, and the
/// text an answer was computed from is the only text that position is valid in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub start: u32,
    pub end: u32,
    pub text: Arc<str>,
}

/// Where a definition is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// In a file with a path.
    File(Location),
    /// In the core prelude, which is compiled into the toolchain and has no file an editor
    /// could open. Named rather than pointed at, so a client is not sent to a path that
    /// does not exist.
    Prelude { name: String },
}

/// What `definition` found, and which kind of use it was found from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// `function`, `local`, `record`, `variant`, `field` or `payload field`.
    pub kind: &'static str,
    pub target: Target,
}

/// What `hover` found: text read from the resolution, not reconstructed from syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub text: String,
    /// The token the answer is about, in the document's text.
    pub start: u32,
    pub end: u32,
}

/// The complete current diagnostic set for one document — possibly empty, which is how a
/// client learns that errors it was shown are gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub path: PathBuf,
    /// The document version the diagnostics are of, or `None` for a document that has just
    /// been closed and whose set is therefore cleared.
    pub version: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
    /// The text the spans are offsets into.
    pub text: Arc<str>,
}

/// One open document.
#[derive(Debug)]
struct Document {
    /// The path the client named, for everything that reports it.
    path: PathBuf,
    version: i32,
    text: Arc<str>,
    analysis: Option<Snapshot>,
}

/// Everything one analysis of one document produced, kept only until the next mutation.
#[derive(Debug)]
struct Snapshot {
    generation: u64,
    version: i32,
    map: SourceMap,
    graph: ModuleGraph,
    analysis: Analysis,
    /// The document's own concrete tree, for finding the token under a position.
    cst: Cst,
    /// The texts of the compilation's files, shared with what is handed out.
    texts: Vec<Arc<str>>,
}

/// The files an editor has open, and what the compiler says about them.
///
/// **Must be driven from a thread with at least
/// [`nazm_core::__syntax::PARSE_STACK_BYTES`] of stack**: analysing parses and checks on the
/// caller's stack and keeps the trees, which are dropped on it too. `nazm lsp` runs it on
/// such a thread; so do its tests.
#[derive(Debug, Default)]
pub struct LanguageService {
    /// By identity path ([`identity`]), so two spellings of one file are one document.
    documents: BTreeMap<PathBuf, Document>,
    generation: u64,
    /// `--source-root`, if it was given: the same override `nazm check` takes.
    source_root: Option<PathBuf>,
}

/// The path a file is identified by here: canonical when it exists, and otherwise its
/// canonical directory joined with its name, so a buffer for a file not yet on disk still
/// matches the path an import would reach it by.
fn identity(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let absolute = root::absolute(path);
    match (absolute.parent(), absolute.file_name()) {
        (Some(dir), Some(name)) => dir
            .canonicalize()
            .map_or_else(|_| absolute.clone(), |d| d.join(name)),
        _ => absolute,
    }
}

/// Open buffers first, then the disk.
struct Overlaid<'a> {
    documents: &'a BTreeMap<PathBuf, Document>,
}

impl Sources for Overlaid<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.documents.get(&identity(path)) {
            Some(d) => Ok(d.text.to_string()),
            None => Disk.read(path),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.documents.contains_key(&identity(path)) || Disk.exists(path)
    }
}

impl LanguageService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A service whose compilations are measured from `root`, as `nazm check
    /// --source-root` is. Without one, each document's root is decided exactly as
    /// `nazm check <that document>` decides it: a `nazm.root` marker, else its directory.
    #[must_use]
    pub fn with_source_root(root: Option<PathBuf>) -> Self {
        Self {
            source_root: root,
            ..Self::default()
        }
    }

    /// The number of open documents.
    #[must_use]
    pub fn open_documents(&self) -> usize {
        self.documents.len()
    }

    /// The current text of the open document at `path`, which every position a client
    /// sends about it is a position in.
    #[must_use]
    pub fn text(&self, path: &Path) -> Option<Arc<str>> {
        self.documents.get(&identity(path)).map(|d| d.text.clone())
    }

    /// What the service is holding: `(documents, analyses, bytes of source text)`.
    ///
    /// For a long-lived process the property that matters is that this is a function of
    /// what is open *now* — one analysis per open document, over the current text of each
    /// file in its compilation — and never of how many edits there have been.
    #[must_use]
    pub fn retained(&self) -> (usize, usize, usize) {
        let analyses = self
            .documents
            .values()
            .filter(|d| d.analysis.is_some())
            .count();
        let bytes = self
            .documents
            .values()
            .map(|d| {
                d.text.len()
                    + d.analysis
                        .as_ref()
                        .map_or(0, |s| s.texts.iter().map(|t| t.len()).sum::<usize>())
            })
            .sum();
        (self.documents.len(), analyses, bytes)
    }

    /// Open `path` at `version` with exactly `text`, which is from now on what every
    /// compilation reads for that file instead of the disk.
    pub fn open(&mut self, path: &Path, version: i32, text: String) -> Vec<Published> {
        self.documents.insert(
            identity(path),
            Document {
                path: path.to_path_buf(),
                version,
                text: text.into(),
                analysis: None,
            },
        );
        self.refresh()
    }

    /// Replace an open document's text. A change to a document that is not open is
    /// treated as opening it, which is what the text now is either way.
    pub fn change(&mut self, path: &Path, version: i32, text: String) -> Vec<Published> {
        self.open(path, version, text)
    }

    /// Close `path`. The disk is authoritative for it again, and its diagnostics are
    /// cleared.
    pub fn close(&mut self, path: &Path) -> Vec<Published> {
        let closed = self.documents.remove(&identity(path));
        let mut out = self.refresh();
        if let Some(d) = closed {
            out.push(Published {
                path: d.path,
                version: None,
                diagnostics: Vec::new(),
                text: d.text,
            });
        }
        out
    }

    /// Re-analyse every open document against the current buffers, and publish each one's
    /// complete diagnostic set.
    fn refresh(&mut self) -> Vec<Published> {
        self.generation += 1;
        let generation = self.generation;
        let keys: Vec<PathBuf> = self.documents.keys().cloned().collect();
        let mut out = Vec::new();
        for key in keys {
            let Some(doc) = self.documents.get(&key) else {
                continue;
            };
            let (path, version) = (doc.path.clone(), doc.version);
            let snapshot = analyse(
                &self.documents,
                self.source_root.as_deref(),
                &path,
                version,
                generation,
            );
            let published = Published {
                path: path.clone(),
                version: Some(version),
                diagnostics: own_diagnostics(&snapshot),
                text: snapshot
                    .texts
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Arc::from("")),
            };
            if let Some(d) = self.documents.get_mut(&key) {
                d.analysis = Some(snapshot);
            }
            out.push(published);
        }
        out
    }

    /// The analysis of `path` at `version`, if it is the current one.
    fn current(&self, path: &Path, version: i32) -> Option<&Snapshot> {
        let doc = self.documents.get(&identity(path))?;
        let s = doc.analysis.as_ref()?;
        (doc.version == version && s.version == version && s.generation == self.generation)
            .then_some(s)
    }

    /// Where the name at byte `offset` of `path`, at `version`, is defined.
    ///
    /// `None` when the position is not on a name, when the document is not at `version`,
    /// or when the compiler's resolution has no identity for that use — a name inside a
    /// body that did not parse, a type annotation, a built-in.
    #[must_use]
    pub fn definition(&self, path: &Path, version: i32, offset: u32) -> Option<Definition> {
        let s = self.current(path, version)?;
        let at = name_at(&s.cst, offset)?;
        let res = &s.analysis.resolution;
        let (kind, span) = defined_at(res, &s.analysis, at)?;
        Some(Definition {
            kind,
            target: s.target(span)?,
        })
    }

    /// What the name at byte `offset` of `path`, at `version`, is — from the resolution:
    /// a function's declared signature, or a local's type. `None` otherwise.
    #[must_use]
    pub fn hover(&self, path: &Path, version: i32, offset: u32) -> Option<Hover> {
        let s = self.current(path, version)?;
        let at = name_at(&s.cst, offset)?;
        let res = &s.analysis.resolution;
        let types = res.types();
        let text = if let Some(Callee::Function(id)) = res.callee(at) {
            let f = res.function(id)?;
            let params: Vec<String> = f.params.iter().map(|t| types.show(*t)).collect();
            format!(
                "fn {}({}) -> {}",
                f.name,
                params.join(", "),
                types.show(f.ret)
            )
        } else {
            let slot = res.var(at)?;
            let f = enclosing(&s.analysis, at)?;
            let l = f.locals.get(usize::try_from(slot).ok()?)?;
            format!(
                "{}{}: {}",
                if l.mutable { "mut " } else { "" },
                l.name,
                types.show(l.ty)
            )
        };
        Some(Hover {
            text,
            start: at.start,
            end: at.end,
        })
    }
}

impl Snapshot {
    /// A definition span as a place a client can go.
    fn target(&self, span: Span) -> Option<Target> {
        let module = self.graph.module_of(span.file)?;
        if Some(module) == self.graph.prelude() {
            return Some(Target::Prelude {
                name: self.map.name(span.file).to_owned(),
            });
        }
        let text = self.texts.get(span.file.index())?.clone();
        Some(Target::File(Location {
            path: root::absolute(Path::new(self.map.name(span.file))),
            start: span.start,
            end: span.end,
            text,
        }))
    }
}

/// Load `path` as the root of a compilation against the open buffers, and analyse it —
/// exactly the compilation `nazm check <path>` would load, with the buffers in place of
/// the files.
fn analyse(
    documents: &BTreeMap<PathBuf, Document>,
    source_root: Option<&Path>,
    path: &Path,
    version: i32,
    generation: u64,
) -> Snapshot {
    let sources = Overlaid { documents };
    // The CLI's rule, unchanged: a declared root beats a marker beats the file's own
    // directory. A root that cannot be decided is reported the way `nazm check` reports it.
    let (loaded, mut early) = match root::decide(path, source_root) {
        Ok(r) => match load::load_from(path, r.path(), &sources) {
            Ok(l) => (l, Vec::new()),
            Err((partial, d)) => (partial, d),
        },
        Err(e) => {
            let text = sources.read(path).unwrap_or_default();
            let map = SourceMap::single(path.display().to_string(), text);
            let graph = ModuleGraph::single();
            (
                load::Loaded { map, graph },
                vec![Diagnostic::error(
                    nazm_diag::codes::MODULE_NOT_FOUND,
                    Span::synthetic(),
                    e,
                )],
            )
        }
    };
    let load::Loaded { map, graph } = loaded;
    // A module, not a program: whether it has a `main` is `nazm check`'s and `nazm run`'s
    // question about a program, and an editor opening a library is not asking it.
    let mut analysis = nazm_core::analyse(&map, &graph, Entry::Absent);
    early.append(&mut analysis.diagnostics);
    analysis.diagnostics = early;
    let texts: Vec<Arc<str>> = map.ids().map(|f| Arc::from(map.text(f))).collect();
    let root_text = texts.first().map_or("", |t| &**t);
    let cst = nazm_core::__syntax::parse_cst(FileId::ROOT, root_text).cst;
    Snapshot {
        generation,
        version,
        map,
        graph,
        analysis,
        cst,
        texts,
    }
}

/// The diagnostics of a document's own compilation that belong to the document: those in
/// its file, and those with no file at all (a module that could not be read), which have
/// nowhere else to be shown.
fn own_diagnostics(s: &Snapshot) -> Vec<Diagnostic> {
    let root = FileId::ROOT;
    s.analysis
        .diagnostics
        .iter()
        .filter(|d| d.primary.span.file == root || d.primary.span == Span::synthetic())
        .cloned()
        .collect()
}

/// The name token at byte `offset` of the document, by the concrete tree.
///
/// The rule, exactly: the identifier whose bytes contain `offset`, or whose last byte is
/// immediately before it — so a cursor just after a name is on the name, as an editor
/// places it after typing. Any other position — whitespace, a comment, punctuation, a
/// keyword, a literal, the end of the file — is on no name. No nearest token is guessed.
fn name_at(cst: &Cst, offset: u32) -> Option<Span> {
    let leaves = cst.leaves();
    let is_name = |k: SyntaxKind| k == SyntaxKind::Ident;
    let inside = leaves
        .iter()
        .find(|l| l.span.start <= offset && offset < l.span.end);
    if let Some(l) = inside {
        if is_name(l.kind) {
            return Some(l.span);
        }
    }
    leaves
        .iter()
        .find(|l| is_name(l.kind) && l.span.end == offset)
        .map(|l| l.span)
}

/// What the use of a name at `at` was resolved to, and where that definition's name is
/// written — one question per kind of identity the resolution records, asked in turn.
fn defined_at(res: &Resolution, a: &Analysis, at: Span) -> Option<(&'static str, Span)> {
    if let Some(Callee::Function(id)) = res.callee(at) {
        return Some(("function", res.function(id)?.name_span));
    }
    if let Some(slot) = res.var(at) {
        let f = enclosing(a, at)?;
        return Some(("local", f.locals.get(usize::try_from(slot).ok()?)?.span));
    }
    if let Some(r) = res.construction(at) {
        return Some(("record", res.records().get(r)?.name_span));
    }
    if let Some(v) = res.variant(at) {
        let e = res.enums().get(v.enumeration)?;
        let variant = e.variants.get(usize::try_from(v.variant).ok()?)?;
        return Some(("variant", variant.name_span));
    }
    if let Some(f) = res.field(at) {
        let r = res.records().get(f.record)?;
        return Some((
            "field",
            r.fields.get(usize::try_from(f.field).ok()?)?.name_span,
        ));
    }
    let p = res.payload(at)?;
    let e = res.enums().get(p.enumeration)?;
    let variant = e.variants.get(usize::try_from(p.variant).ok()?)?;
    let field = variant.fields.get(usize::try_from(p.field).ok()?)?;
    Some(("payload field", field.name_span))
}

/// The function whose body `span` is in, by the resolution's own link from a definition
/// to its declaration.
fn enclosing(a: &Analysis, span: Span) -> Option<&FnDef> {
    a.resolution.functions().iter().find(|f| {
        a.program
            .functions
            .get(f.decl as usize)
            .is_some_and(|decl| {
                decl.span.file == span.file
                    && decl.span.start <= span.start
                    && span.end <= decl.span.end
            })
    })
}

#[cfg(test)]
mod tests {
    /// The structural half of "not a second compiler": this module reads the compiler's
    /// answers and computes none of its own. It never tokenises or parses source to learn
    /// what a name means, and it keeps no table from names to definitions — a use goes to
    /// its definition by the identity the resolution recorded, or nowhere. The layering
    /// gate (`xtask/src/rules.rs`) keeps `nazm-syntax` and `nazm-sema` out of reach, and the
    /// `one resolver` gate keeps the whole-program name search out of every crate but the
    /// checker's diagnostics; this keeps the re-exports that remain from being used to
    /// rebuild them.
    #[test]
    fn the_service_computes_no_semantics_of_its_own() {
        let source = include_str!("service.rs");
        let code: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "lex(",
            "lex_in(",
            "parse_source(",
            "parse_file(",
            "parse_map(",
            "check(",
            "HashMap<String",
            "BTreeMap<String",
            ".text ==",
            "definition(module",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in the service: it would be computing an answer the compiler owns"
            );
        }
    }
}
