//! The language service: compiler answers for the files an editor has open. N16.
//!
//! # What it is, and what it is not
//!
//! A long-lived owner of **open documents** — their exact current text and version — that
//! answers four questions about them: what is wrong ([`Published`] diagnostics), where a
//! name is defined ([`LanguageService::definition`]), what a name is
//! ([`LanguageService::hover`]), and where else the entity a name refers to is used
//! ([`LanguageService::references`], N17). It speaks no protocol: positions are byte
//! offsets into a document's text and every answer names files by path. `nazm lsp` is an
//! adapter over it.
//!
//! **It is not a second compiler.** It has no lexer, parser, symbol table, resolver or
//! checker of its own. Every answer comes from three things the compiler already owns:
//!
//! - [`crate::load::load_from`] — the loader every command uses, reading an open buffer in
//!   place of the file on disk;
//! - [`nazm_core::analyse`] — the same parse and check every command runs;
//! - [`nazm_core::Resolution`] — what that check decided each name refers to, by identity,
//!   and [`ReferenceIndex`], the same answers read backwards: each entity's declaration and
//!   uses, derived from the resolution and from nothing else.
//!
//! The concrete tree ([`nazm_core::__syntax::Cst`]) finds *which token* a position is on. What that
//! token *means* is only ever read from the resolution. A query that the resolution cannot
//! answer — a name in a body that did not parse, a built-in, a type parameter — has no
//! answer; nothing here guesses from spelling.
//!
//! Definition and references are one lookup: the occurrence under the position is taken to
//! its entity by the index, and definition is that entity's declaration while references
//! are its uses. They cannot disagree about what a name means, because neither decides it.
//!
//! # Which program a references query covers
//!
//! The **current snapshot**: every compilation the service holds — one rooted at each open
//! document, loaded exactly as `nazm check <that document>` would load it — that contains
//! the file declaring the entity, with the same text. An entity's uses can only be in
//! modules that import the one declaring it, so this reaches every use in every program
//! the service is analysing. A file that no open document's compilation loads is in no
//! program being analysed, and is not searched: there is no scan of the disk for `.nz`
//! files, and no index of anything that is not open or imported.
//!
//! Identity does not cross compilations — every id is session-local to one — so an entity
//! is carried from one compilation to another by **where it is declared**: the same bytes
//! of the same file, analysed from the same text, are the same declaration in each. That is
//! a position, read by each compilation's own index; it is not a name, and nothing is
//! resolved again.
//!
//! # What is recomputed
//!
//! Everything semantic, on every change, for every open document: each is loaded as the
//! root of a compilation — exactly the compilation `nazm check <that file>` would load —
//! and checked. **Its own text is not parsed again** (N76): each open document keeps the
//! compiler's [`Revision`] of its text, moved by every ranged edit an editor sends
//! ([`LanguageService::edit`]), and its compilation takes the root file's trees and syntax
//! diagnostics from that revision. A revision equals a fresh parse of its text by contract
//! (`architecture.md` §7.77), so this changes what is computed, not what is answered. Every
//! other file of the compilation is parsed in full; there is no graph maintenance, no reuse
//! of semantic work between versions, and no persistent cache read or written (an unsaved
//! buffer is not state the cache may record). `docs/performance.md` measures what that
//! costs.
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

use nazm_core::__syntax::{Cst, EditError, ParseStats, Revision, SyntaxKind, TextEdit};
use nazm_core::sema::module::ModuleGraph;
use nazm_core::sema::{Callee, ReferenceIndex};
use nazm_core::{Analysis, Entry};
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
    /// In a module compiled into the toolchain — the core prelude, or a standard module
    /// (N45) — which has no file an editor could open. Named rather than pointed at, so a
    /// client is not sent to a path that does not exist.
    Prelude { name: String },
}

/// What `definition` found, and which kind of use it was found from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// `function`, `local`, `record`, `variant`, `field` or `payload field`.
    pub kind: &'static str,
    pub target: Target,
}

/// What `references` found: the entity's declaration, and every use of it, apart.
///
/// Apart because they are different facts — a definition is not a use of itself — and a
/// consumer that wants them together (an editor's `includeDeclaration`) says so with
/// [`Self::locations`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct References {
    /// `function`, `local`, `record`, `variant`, `field` or `payload field`.
    pub kind: &'static str,
    /// Where the entity is declared, when that is a file a client can open. `None` for an
    /// entity of the core prelude, which has no file.
    pub declaration: Option<Location>,
    /// Every use, ordered by file, then start, then end, each once. Uses inside the core
    /// prelude are not among them, for the same reason.
    pub uses: Vec<Location>,
}

impl References {
    /// The uses, and the declaration too if `include_declaration`, in one canonical order.
    #[must_use]
    pub fn locations(&self, include_declaration: bool) -> Vec<Location> {
        let mut all = self.uses.clone();
        if include_declaration {
            if let Some(d) = &self.declaration {
                all.push(d.clone());
            }
        }
        all.sort_by(|a, b| (&a.path, a.start, a.end).cmp(&(&b.path, b.start, b.end)));
        all
    }
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
pub(crate) struct Document {
    /// The path the client named, for everything that reports it.
    pub(crate) path: PathBuf,
    pub(crate) version: i32,
    pub(crate) text: Arc<str>,
    /// The compiler's parse of `text` as the root file of the document's compilation,
    /// moved by [`LanguageService::edit`] (N76).
    pub(crate) revision: Revision,
    pub(crate) analysis: Option<Snapshot>,
}

/// Everything one analysis of one document produced, kept only until the next mutation.
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) generation: u64,
    version: i32,
    pub(crate) map: SourceMap,
    pub(crate) graph: ModuleGraph,
    pub(crate) analysis: Analysis,
    /// Every entity of the compilation with its declaration and uses, read from
    /// `analysis.resolution` once, when the analysis is made, and dropped with it.
    pub(crate) index: ReferenceIndex,
    /// The document's own concrete tree, for finding the token under a position.
    pub(crate) cst: Cst,
    /// The texts of the compilation's files, shared with what is handed out.
    pub(crate) texts: Vec<Arc<str>>,
    /// Which file each of the compilation's files is, by [`FileId`] index, so that one file
    /// can be recognised in two compilations.
    pub(crate) files: Vec<FileKey>,
    /// Whether the root file's parse was the document's revision rather than a new one.
    pub(crate) reused_root: bool,
}

/// A file, as the same file in any compilation: its [`identity`] path, or the core
/// prelude, which every compilation has and none reads from a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FileKey {
    Path(PathBuf),
    Prelude,
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
    pub(crate) documents: BTreeMap<PathBuf, Document>,
    pub(crate) generation: u64,
    /// `--source-root`, if it was given: the same override `nazm check` takes.
    pub(crate) source_root: Option<PathBuf>,
}

/// The path a file is identified by here: canonical when it exists, and otherwise its
/// canonical directory joined with its name, so a buffer for a file not yet on disk still
/// matches the path an import would reach it by.
pub(crate) fn identity(path: &Path) -> PathBuf {
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

/// Open buffers first, then the disk — and, for a rename's candidate program (N18), one
/// file's proposed text before either.
pub(crate) struct Overlaid<'a> {
    pub(crate) documents: &'a BTreeMap<PathBuf, Document>,
    /// `(identity path, text)`: a file as a rename would leave it, read in place of its
    /// buffer or its disk contents. Never written anywhere.
    pub(crate) replaced: Option<(&'a Path, &'a str)>,
    /// Several files as a workspace rename would leave them (N89), by identity path.
    pub(crate) many: &'a [(PathBuf, String)],
}

impl Sources for Overlaid<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        let key = identity(path);
        if let Some((at, text)) = self.replaced {
            if at == key {
                return Ok(text.to_owned());
            }
        }
        if let Some((_, text)) = self.many.iter().find(|(at, _)| *at == key) {
            return Ok(text.clone());
        }
        match self.documents.get(&key) {
            Some(d) => Ok(d.text.to_string()),
            None => Disk.read(path),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        let key = identity(path);
        self.replaced.is_some_and(|(at, _)| at == key)
            || self.many.iter().any(|(at, _)| *at == key)
            || self.documents.contains_key(&key)
            || Disk.exists(path)
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

    /// What the reference indexes are holding: `(entities, uses)`, over every analysis.
    ///
    /// Like [`Self::retained`], a function of what is open and what it imports now, never
    /// of the history of edits: each analysis's index is replaced with it.
    #[must_use]
    pub fn indexed(&self) -> (usize, usize) {
        self.documents
            .values()
            .filter_map(|d| d.analysis.as_ref())
            .fold((0, 0), |(t, u), s| {
                (t + s.index.targets(), u + s.index.use_count())
            })
    }

    /// What the scope traces are holding: `(frames, bindings)`, over every analysis (N19).
    /// A function of what is open now, like [`Self::retained`] and [`Self::indexed`].
    #[must_use]
    pub fn scoped(&self) -> (usize, usize) {
        self.documents
            .values()
            .filter_map(|d| d.analysis.as_ref())
            .fold((0, 0), |(f, b), s| {
                let (sf, sb) = s.analysis.resolution.scopes().size();
                (f + sf, b + sb)
            })
    }

    /// How many checked calls the analyses are holding, over every analysis (N20). A
    /// function of what is open now, like [`Self::retained`].
    #[must_use]
    pub fn calls(&self) -> usize {
        self.documents
            .values()
            .filter_map(|d| d.analysis.as_ref())
            .map(|s| s.analysis.resolution.checked_calls().count())
            .sum()
    }

    /// How many unresolved member names the analyses are holding, over every analysis
    /// (N21): the only structure fact the checker keeps for tooling, written where a
    /// field or variant lookup failed. A function of what is open now.
    #[must_use]
    pub fn unresolved_members(&self) -> usize {
        self.documents
            .values()
            .filter_map(|d| d.analysis.as_ref())
            .map(|s| s.analysis.resolution.unresolved_member_count())
            .sum()
    }

    /// Open `path` at `version` with exactly `text`, which is from now on what every
    /// compilation reads for that file instead of the disk.
    pub fn open(&mut self, path: &Path, version: i32, text: String) -> Vec<Published> {
        let revision = Revision::new(FileId::ROOT, text.clone());
        self.documents.insert(
            identity(path),
            Document {
                path: path.to_path_buf(),
                version,
                text: text.into(),
                revision,
                analysis: None,
            },
        );
        self.refresh()
    }

    /// Apply `edits`, in order, to an open document's text, as `version`: each a byte range
    /// of the text as the edits before it left it (N76). The document's revision moves with
    /// each — reparsing only what an edit can have changed — and every open document is
    /// analysed again, as for [`LanguageService::change`].
    ///
    /// # Errors
    ///
    /// `None` when the document is not open; the edit's error when one is not a range of
    /// the text it applies to on character boundaries. Nothing changes then, not even for
    /// the edits before it: the editor's text and this one's have diverged, and the editor
    /// is expected to send the whole document.
    pub fn edit(
        &mut self,
        path: &Path,
        version: i32,
        edits: &[TextEdit],
    ) -> Result<Vec<Published>, Option<EditError>> {
        let key = identity(path);
        let doc = self.documents.get(&key).ok_or(None)?;
        let mut revision = doc.revision.clone();
        for e in edits {
            revision = revision.edit(e).map_err(Some)?;
        }
        if let Some(doc) = self.documents.get_mut(&key) {
            doc.text = Arc::from(revision.text());
            doc.version = version;
            doc.revision = revision;
        }
        Ok(self.refresh())
    }

    /// What the last parse of an open document's text did (N76): whether it was full, and
    /// how much an edit reused.
    #[must_use]
    pub fn parse_stats(&self, path: &Path) -> Option<ParseStats> {
        self.documents
            .get(&identity(path))
            .map(|d| d.revision.stats())
    }

    /// Whether the current analysis of an open document took its root file's parse from the
    /// document's revision (N76).
    #[must_use]
    pub fn reused_root_parse(&self, path: &Path) -> Option<bool> {
        self.documents
            .get(&identity(path))
            .and_then(|d| d.analysis.as_ref())
            .map(|s| s.reused_root)
    }

    /// Open `path` at version 0 with its current contents on disk — for a one-shot client
    /// that has no buffers, such as `nazm context` and `nazm-mcp` (N27), which open their
    /// root this way on every request so that each answer is of the disk as it is then.
    ///
    /// # Errors
    ///
    /// The file could not be read.
    pub fn open_from_disk(&mut self, path: &Path) -> io::Result<Vec<Published>> {
        let text = Disk.read(path)?;
        Ok(self.open(path, 0, text))
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
            let sources = Overlaid {
                documents: &self.documents,
                replaced: None,
                many: &[],
            };
            let snapshot = analyse(
                &sources,
                self.source_root.as_deref(),
                &path,
                version,
                generation,
                Some(&doc.revision),
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
    pub(crate) fn current(&self, path: &Path, version: i32) -> Option<&Snapshot> {
        let doc = self.documents.get(&identity(path))?;
        let s = doc.analysis.as_ref()?;
        (doc.version == version && s.version == version && s.generation == self.generation)
            .then_some(s)
    }

    /// Where the name at byte `offset` of `path`, at `version`, is defined.
    ///
    /// `None` when the position is not on a name, when the document is not at `version`,
    /// or when the compiler's resolution has no identity for that use — a name inside a
    /// body that did not parse, a built-in, a type parameter.
    #[must_use]
    pub fn definition(&self, path: &Path, version: i32, offset: u32) -> Option<Definition> {
        let s = self.current(path, version)?;
        let at = name_at(&s.cst, offset)?;
        let entity = s.index.target(at)?;
        Some(Definition {
            kind: entity.kind(),
            target: s.target(s.index.declaration(entity)?)?,
        })
    }

    /// Every use of the entity the name at byte `offset` of `path`, at `version`, refers
    /// to — or is the declaration of — and where that entity is declared.
    ///
    /// `None` exactly when [`Self::definition`] would have no entity to go to: not on a
    /// name, not the current version, or a name the resolution identified as nothing — a
    /// built-in, a type parameter, a name in a body that did not parse. An entity with no
    /// uses is `Some`, with none.
    ///
    /// What *is* the entity is decided by `path`'s own compilation. Its uses are gathered
    /// from every current compilation that contains the file declaring it (see the module
    /// documentation), each reading its own index.
    #[must_use]
    pub fn references(&self, path: &Path, version: i32, offset: u32) -> Option<References> {
        let s = self.current(path, version)?;
        let at = name_at(&s.cst, offset)?;
        let entity = s.index.target(at)?;
        // The anchor: where the entity is declared, as a file any compilation may have.
        // An entity with no declaration in source (the `out` a `main` is given, R1 5b.1) is
        // anchored where it was asked about, which is at least in this compilation.
        let anchor = s.index.declaration(entity).unwrap_or(at);
        let file = s.files.get(anchor.file.index())?;
        let text = s.texts.get(anchor.file.index())?;
        let declaration = s
            .index
            .declaration(entity)
            .and_then(|d| match s.target(d)? {
                Target::File(l) => Some(l),
                Target::Prelude { .. } => None,
            });

        let mut uses: BTreeMap<(FileKey, u32, u32), Location> = BTreeMap::new();
        for other in self.documents.values().filter_map(|d| d.analysis.as_ref()) {
            if other.generation != self.generation {
                continue;
            }
            // The declaring file, in this compilation, with the text it was analysed
            // from — or this compilation does not have the entity at all.
            let Some(f) = other
                .files
                .iter()
                .zip(&other.texts)
                .position(|(k, t)| k == file && **t == **text)
            else {
                continue;
            };
            let there = Span::new(FileId::from_index(f), anchor.start, anchor.end);
            let Some(same) = other.index.target(there) else {
                continue;
            };
            for u in other.index.uses(same) {
                let (Some(key), Some(Target::File(l))) =
                    (other.files.get(u.file.index()), other.target(*u))
                else {
                    continue;
                };
                uses.entry((key.clone(), u.start, u.end)).or_insert(l);
            }
        }
        Some(References {
            kind: entity.kind(),
            declaration,
            uses: uses.into_values().collect(),
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
            let label = crate::Signature::of_function(res, id)?
                .render(res, None)
                .label;
            // A function that declares no effects has what its body requires, which is
            // not a declaration and so is not in the signature — but is the compiler's.
            match res.function(id) {
                Some(f) if f.declared_effects.is_none() => match f.inferred_effects {
                    Some(e) => format!("{label}\ninferred effects: {e}"),
                    None => label,
                },
                _ => label,
            }
        } else {
            let local = res.local(at)?;
            let f = res.function(local.function)?;
            let l = f.locals.get(usize::try_from(local.slot).ok()?)?;
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
    pub(crate) fn target(&self, span: Span) -> Option<Target> {
        let module = self.graph.module_of(span.file)?;
        if Some(module) == self.graph.prelude()
            || self
                .graph
                .key(module)
                .is_some_and(nazm_core::sema::ModuleKey::is_std)
        {
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
pub(crate) fn analyse(
    sources: &Overlaid<'_>,
    source_root: Option<&Path>,
    path: &Path,
    version: i32,
    generation: u64,
    revision: Option<&Revision>,
) -> Snapshot {
    // The CLI's rule, unchanged: a declared root beats a marker beats the file's own
    // directory. A root that cannot be decided is reported the way `nazm check` reports it.
    let (loaded, mut early) = match root::decide(path, source_root) {
        Ok(r) => match load::load_from(path, r.path(), sources) {
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
    let (mut analysis, reused_root) =
        nazm_core::analyse_parsed(&map, &graph, Entry::Absent, revision);
    early.append(&mut analysis.diagnostics);
    analysis.diagnostics = early;
    let index = ReferenceIndex::of(&analysis.resolution);
    let texts: Vec<Arc<str>> = map.ids().map(|f| Arc::from(map.text(f))).collect();
    let files = map
        .ids()
        .map(|f| match graph.module_of(f) {
            Some(m) if Some(m) == graph.prelude() => FileKey::Prelude,
            _ => FileKey::Path(identity(Path::new(map.name(f)))),
        })
        .collect();
    let root_text = texts.first().map_or("", |t| &**t);
    // The revision's tree when its parse was the one analysed; otherwise the root's text,
    // parsed (N76).
    let cst = match revision {
        Some(r) if reused_root => r.cst().clone(),
        _ => nazm_core::__syntax::parse_cst(FileId::ROOT, root_text).cst,
    };
    Snapshot {
        generation,
        version,
        map,
        graph,
        analysis,
        index,
        cst,
        texts,
        files,
        reused_root,
    }
}

/// The diagnostics of a document's own compilation that belong to the document: those in
/// its file, and those with no file at all (a module that could not be read), which have
/// nowhere else to be shown.
fn own_diagnostics(s: &Snapshot) -> Vec<Diagnostic> {
    let root = FileId::ROOT;
    // The checker's warnings too (R1b, L15), at their own severity.
    s.analysis
        .diagnostics
        .iter()
        .cloned()
        .chain(nazm_core::warnings(&s.analysis.resolution))
        .filter(|d| d.primary.span.file == root || d.primary.span == Span::synthetic())
        .collect()
}

/// The name token at byte `offset` of the document, by the concrete tree.
///
/// The rule, exactly: the identifier whose bytes contain `offset`, or whose last byte is
/// immediately before it — so a cursor just after a name is on the name, as an editor
/// places it after typing. Any other position — whitespace, a comment, punctuation, a
/// keyword, a literal, the end of the file — is on no name. No nearest token is guessed.
pub(crate) fn name_at(cst: &Cst, offset: u32) -> Option<Span> {
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
