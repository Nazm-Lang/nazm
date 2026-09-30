//! Compact machine diagnostics and progressive disclosure: every current diagnostic of a
//! root compilation as a small index of compiler-owned facts, and one selected
//! diagnostic's full detail on request. `nazm.diagnostic-index/1`, `nazm.diagnostic-detail/1`.
//! N30.
//!
//! # The contract
//!
//! **Compact diagnostics never infer structured semantics from diagnostic prose.** An
//! index entry is the diagnostic's code, severity, primary place, owning definition and fix
//! counts — each read from a structured field the compiler set — and none of its message,
//! help, label text, fix description or precondition. The compiler's [`Diagnostic`] carries
//! no typed expected/actual facts today, so none are reported: `facts` is `unavailable`,
//! never a field reconstructed from a sentence. Detail is the diagnostic exactly as
//! `nazm.diagnostic/1` publishes it — the canonical prose, unchanged — with its places in
//! their files, its owner and, for each fix, whether N29 can plan it.
//!
//! **A compiler fix and an N29 semantic patch are distinct capabilities.** Every fix a
//! diagnostic carries is counted; a patch is available only where N29 would plan one — a
//! compilation with a snapshot to bind to, and the fix plannable by N24 — so a syntax
//! diagnostic's fix is a compiler fix with no semantic patch. The index builds no patch;
//! detail checks N24's plan for its one diagnostic and builds no patch either.
//!
//! # Which diagnostics, and which state
//!
//! Every diagnostic of the root's compilation as the service analyses it — the same
//! analysis N27–N29 read, from disk, the root and everything it imports and no other file.
//! It is a module's analysis, as the language server's is, so a missing `main` (`N0204`),
//! a program's question, is not among them. Syntax diagnostics are: an index needs no
//! semantic snapshot. Its state is bound twice, by two fields that never change meaning:
//!
//! | Field | Is | Present |
//! | --- | --- | --- |
//! | `state.source` | BLAKE3 over the root's identity and every loaded source file's root-relative path and exact bytes, in load order | always |
//! | `state.snapshot` | BLAKE3 of the root's `nazm.snapshot/3` (N28) | when the compilation has one — not after syntax recovery or a missing module |
//!
//! A detail request names the index's `state.source`; any other value is stale, and an id
//! is resolved only within the state it came from — so an id never selects another
//! diagnostic that has come to occupy its place.
//!
//! # Identity and order
//!
//! Entries are ordered by primary file, start, end and code, then by their secondary and
//! fix places, then by the compiler's own order, which is deterministic. An id is
//! `code@file:start-end#n`, `n` counting earlier entries with the same code and primary
//! place: two diagnostics the compiler's structure cannot tell apart are told apart by that
//! ordinal and by nothing else. No message, help or description reaches an id. Places are
//! root-relative files and UTF-8 byte offsets into them, translated from the compiler's
//! spans by the source map every other answer uses; a diagnostic with no place (a root that
//! could not be read) has none.
//!
//! # Owners
//!
//! A function, record or enum with a durable key whose declaration, by N22's outline,
//! contains the primary span — and only in a compilation that needed no syntax recovery and
//! loaded every module, where the outline is of the source as written. Otherwise, and for
//! anything outside every declaration, there is no owner; none is guessed from a name.

use std::collections::BTreeMap;
use std::path::Path;

use nazm_diag::{Applicability, Diagnostic};
use nazm_span::{FileId, Span};
use serde::Serialize;

use crate::context::{identity_of, incomplete, Paths, SourceSpan};
use crate::patch::digest;
use crate::service::{identity, FileKey, LanguageService, Snapshot};
use crate::snapshot::{RootIdentity, COMPILER};
use crate::symbols::{declared_in, SymbolKind};

/// The schema every index declares.
pub const INDEX_SCHEMA: &str = "nazm.diagnostic-index/1";
/// The schema every detail declares.
pub const DETAIL_SCHEMA: &str = "nazm.diagnostic-detail/1";

/// An index, or why there is none, exactly as `nazm.diagnostic-index/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IndexResult {
    Available(Box<DiagnosticIndex>),
    Unavailable(NoResult),
}

/// A detail, or why there is none, exactly as `nazm.diagnostic-detail/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DetailResult {
    Available(Box<DiagnosticDetail>),
    Unavailable(NoResult),
}

/// Every current diagnostic of one root compilation, compactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticIndex {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    pub root: RootIdentity,
    pub state: State,
    pub counts: Counts,
    /// In the canonical order; each id once.
    pub diagnostics: Vec<Summary>,
}

/// What an index or a detail is of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct State {
    /// BLAKE3 over the root's identity and every loaded file's path and bytes.
    pub source: String,
    /// BLAKE3 of the root's `nazm.snapshot/3`; `None` where the compilation has no snapshot.
    pub snapshot: Option<String>,
}

/// How many diagnostics, and how many fixes of each applicability they carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub total: usize,
    /// Diagnostics of severity `error` — the one severity the compiler has.
    pub error: usize,
    /// Diagnostics carrying at least one fix.
    pub with_fixes: usize,
    pub automatic_fixes: usize,
    pub needs_review_fixes: usize,
}

/// One diagnostic, compactly: no prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: String,
    pub code: String,
    pub severity: &'static str,
    /// Where it is, in its file; `None` for a diagnostic with no place.
    pub primary: Option<SourceSpan>,
    pub owner: Option<Owner>,
    pub fixes: FixCounts,
}

/// The durable definition a diagnostic is inside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Owner {
    pub identity: String,
    /// `function`, `record` or `enum`.
    pub kind: &'static str,
    pub name: String,
}

/// A diagnostic's compiler fixes, by applicability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FixCounts {
    pub total: usize,
    pub automatic: usize,
    pub needs_review: usize,
}

/// One diagnostic in full, as requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticDetail {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    pub root: RootIdentity,
    pub state: State,
    pub id: String,
    /// Exactly as `nazm.diagnostic/1` publishes it, prose and all: spans are offsets into
    /// the concatenation of the compilation's files, as that schema defines them.
    pub diagnostic: nazm_diag::wire::PublishedDiagnostic,
    /// The same spans, each in its own file: `primary`, then `secondary` and `fixes` in the
    /// diagnostic's order. `None` for a span with no place.
    pub locations: Locations,
    pub owner: Option<Owner>,
    /// Typed expected/actual facts: the compiler records none for any diagnostic today.
    pub facts: &'static str,
    /// How many fixes the compiler attached, and how many N29 can plan as patches.
    pub compiler_fixes: usize,
    pub semantic_patches: usize,
    /// Each compiler fix, in order, and the `nazm.semantic_patch` request that plans it —
    /// `None` where N29 cannot.
    pub fixes: Vec<FixDetail>,
}

/// Every span of a diagnostic, in its own file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Locations {
    pub primary: Option<SourceSpan>,
    pub secondary: Vec<Option<SourceSpan>>,
    pub fixes: Vec<Option<SourceSpan>>,
}

/// One compiler fix, and whether it is a semantic patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixDetail {
    /// Its index among the diagnostic's fixes.
    pub fix: usize,
    /// `automatic` or `needs_review`, exactly as the compiler attached it.
    pub applicability: &'static str,
    pub semantic_patch: Option<PatchSelector>,
}

/// The `diagnostic_fix` request N29 plans this fix from — a selector, never a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchSelector {
    pub file: String,
    pub byte_offset: u32,
    pub fix: usize,
}

/// Why there is no index or detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoResult {
    pub schema: &'static str,
    /// Always `unavailable`.
    pub status: &'static str,
    pub reason: NoDiagnostics,
}

/// Why there is no index or detail. There is never an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoDiagnostics {
    /// The root is not open at that version.
    NotCurrent,
    /// The root module has no durable key.
    NoDurableRoot,
    /// The requested state is not the current one: the sources have changed since.
    StaleState,
    /// No current diagnostic has that id.
    NoSuchDiagnostic,
}

impl NoDiagnostics {
    #[must_use]
    pub const fn index(self) -> IndexResult {
        IndexResult::Unavailable(self.result(INDEX_SCHEMA))
    }

    #[must_use]
    pub const fn detail(self) -> DetailResult {
        DetailResult::Unavailable(self.result(DETAIL_SCHEMA))
    }

    const fn result(self, schema: &'static str) -> NoResult {
        NoResult {
            schema,
            status: "unavailable",
            reason: self,
        }
    }
}

impl IndexResult {
    /// The one canonical serialisation.
    ///
    /// # Panics
    ///
    /// Never: every field is plain data.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an index is plain data")
    }
}

impl DetailResult {
    /// The one canonical serialisation.
    ///
    /// # Panics
    ///
    /// Never: every field is plain data.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a detail is plain data")
    }
}

/// The current diagnostics, identified and ordered, and the state they are of.
pub(crate) struct Current<'a> {
    pub(crate) s: &'a Snapshot,
    pub(crate) paths: Paths,
    pub(crate) root: RootIdentity,
    pub(crate) state: State,
    /// Canonical order.
    pub(crate) entries: Vec<Entry<'a>>,
}

pub(crate) struct Entry<'a> {
    pub(crate) id: String,
    pub(crate) d: &'a Diagnostic,
    pub(crate) primary: Option<SourceSpan>,
    /// Its secondary and fix places, in its own order: with `primary` and the code, the
    /// structure the canonical order and a command summary's matching read (N31).
    pub(crate) secondary: Vec<Option<SourceSpan>>,
    pub(crate) fixes: Vec<Option<SourceSpan>>,
    pub(crate) owner: Option<Owner>,
    /// Earlier diagnostics, in the compiler's order, with the same code and primary span.
    pub(crate) compiler_ordinal: usize,
}

/// A diagnostic's place in the canonical order, from its structure alone: primary file,
/// start and end, code, then secondary and fix places. Ties keep the order they came in.
pub(crate) type Structure = (
    Option<(String, u32, u32)>,
    String,
    Vec<Option<SourceSpan>>,
    Vec<Option<SourceSpan>>,
);

pub(crate) fn structure(
    primary: &Option<SourceSpan>,
    code: &str,
    secondary: &[Option<SourceSpan>],
    fixes: &[Option<SourceSpan>],
) -> Structure {
    (
        primary.as_ref().map(|p| (p.file.clone(), p.start, p.end)),
        code.to_owned(),
        secondary.to_vec(),
        fixes.to_vec(),
    )
}

impl LanguageService {
    /// Every current diagnostic of the compilation rooted at the open document `root`, at
    /// `version`, compactly.
    #[must_use]
    pub fn diagnostic_index(&self, root: &Path, version: i32) -> IndexResult {
        let c = match self.current_diagnostics(root, version) {
            Ok(c) => c,
            Err(why) => return why.index(),
        };
        let mut counts = Counts {
            total: c.entries.len(),
            error: 0,
            with_fixes: 0,
            automatic_fixes: 0,
            needs_review_fixes: 0,
        };
        let diagnostics = c
            .entries
            .iter()
            .map(|e| {
                let fixes = fix_counts(e.d);
                counts.error += usize::from(matches!(e.d.severity, nazm_diag::Severity::Error));
                counts.with_fixes += usize::from(fixes.total > 0);
                counts.automatic_fixes += fixes.automatic;
                counts.needs_review_fixes += fixes.needs_review;
                Summary {
                    id: e.id.clone(),
                    code: e.d.code.to_string(),
                    severity: severity(e.d),
                    primary: e.primary.clone(),
                    owner: e.owner.clone(),
                    fixes,
                }
            })
            .collect();
        IndexResult::Available(Box::new(DiagnosticIndex {
            schema: INDEX_SCHEMA,
            status: "available",
            root: c.root,
            state: c.state,
            counts,
            diagnostics,
        }))
    }

    /// The diagnostic `id` of the index whose `state.source` is `state`, in full — if the
    /// sources are still exactly that state.
    pub fn diagnostic_detail(
        &mut self,
        root: &Path,
        version: i32,
        id: &str,
        state: &str,
    ) -> DetailResult {
        let found = {
            let c = match self.current_diagnostics(root, version) {
                Ok(c) => c,
                Err(why) => return why.detail(),
            };
            if c.state.source != state {
                return NoDiagnostics::StaleState.detail();
            }
            let Some(e) = c.entries.iter().find(|e| e.id == id) else {
                return NoDiagnostics::NoSuchDiagnostic.detail();
            };
            let span = |at: Span| {
                (at != Span::synthetic())
                    .then(|| c.paths.span(at))
                    .flatten()
            };
            let d = e.d.clone();
            let primary_file = c.s.files.get(d.primary.span.file.index()).cloned();
            let text = c.s.texts.get(d.primary.span.file.index()).cloned();
            (
                DiagnosticDetail {
                    schema: DETAIL_SCHEMA,
                    status: "available",
                    root: c.root.clone(),
                    state: c.state.clone(),
                    id: e.id.clone(),
                    diagnostic: d.published(&c.s.map),
                    locations: Locations {
                        primary: e.primary.clone(),
                        secondary: d.secondary.iter().map(|l| span(l.span)).collect(),
                        fixes: d.fixes.iter().map(|f| span(f.span)).collect(),
                    },
                    owner: e.owner.clone(),
                    facts: "unavailable",
                    compiler_fixes: d.fixes.len(),
                    semantic_patches: 0,
                    fixes: d
                        .fixes
                        .iter()
                        .enumerate()
                        .map(|(fix, f)| FixDetail {
                            fix,
                            applicability: applicability(f.applicability),
                            semantic_patch: None,
                        })
                        .collect(),
                },
                d,
                e.compiler_ordinal,
                primary_file,
                text,
                c.state.snapshot.is_some(),
            )
        };
        let (mut detail, d, ordinal, primary_file, text, bound) = found;
        // N29's rule for a diagnostic fix: a compilation with a snapshot to bind to, and the
        // fix one N24 plans. Checked for this diagnostic alone, and no patch is built.
        if let (true, Some(FileKey::Path(key)), Some(text), Some(file)) =
            (bound, primary_file, text, detail.locations.primary.clone())
        {
            let path = key;
            let doc = match self.documents.get(&identity(&path)) {
                Some(d) => Some((d.path.clone(), d.version)),
                None => {
                    let _ = self.open(&path, 0, text.to_string());
                    Some((path.clone(), 0))
                }
            };
            if let Some((doc, v)) = doc {
                let selectors = self.patch_selectors(&doc, v, &d, ordinal);
                for (f, sel) in detail.fixes.iter_mut().zip(selectors) {
                    f.semantic_patch = sel.map(|fix| PatchSelector {
                        file: file.file.clone(),
                        byte_offset: d.primary.span.start,
                        fix,
                    });
                }
            }
        }
        detail.semantic_patches = detail
            .fixes
            .iter()
            .filter(|f| f.semantic_patch.is_some())
            .count();
        DetailResult::Available(Box::new(detail))
    }

    /// For each of `d`'s fixes, its index among the N24 plans at `d`'s primary start in the
    /// open document `doc` — N29's `diagnostic_fix` selector — or `None` where N24 plans no
    /// such edit or the edit would change nothing. `ordinal` says which of the diagnostics
    /// with `d`'s code and primary span it is, in the compiler's order.
    fn patch_selectors(
        &self,
        doc: &Path,
        version: i32,
        d: &Diagnostic,
        ordinal: usize,
    ) -> Vec<Option<usize>> {
        let at = d.primary.span;
        let Ok(plans) = self.fix_plans(doc, version, at.start, at.start) else {
            return vec![None; d.fixes.len()];
        };
        // The distinct diagnostics among the plans with the same code and primary span, in
        // order: the `ordinal`th is `d`.
        let mut seen: Vec<&Diagnostic> = Vec::new();
        let mut out = vec![None; d.fixes.len()];
        for (j, p) in plans.iter().enumerate() {
            let q = &p.diagnostic;
            if q.code != d.code || (q.primary.span.start, q.primary.span.end) != (at.start, at.end)
            {
                continue;
            }
            let n = match seen.iter().position(|x| **x == *q) {
                Some(n) => n,
                None => {
                    seen.push(q);
                    seen.len() - 1
                }
            };
            if n != ordinal {
                continue;
            }
            let empty = p.file.edits.iter().all(|e| e.expected == e.replacement);
            if let Some(slot) = out.get_mut(p.index) {
                if !empty {
                    *slot = Some(j);
                }
            }
        }
        out
    }

    /// The root's current diagnostics, identified and in canonical order.
    pub(crate) fn current_diagnostics(
        &self,
        root: &Path,
        version: i32,
    ) -> Result<Current<'_>, NoDiagnostics> {
        let s = self
            .current(root, version)
            .ok_or(NoDiagnostics::NotCurrent)?;
        let module = s
            .graph
            .module_of(FileId::ROOT)
            .and_then(|m| s.graph.key(m))
            .ok_or(NoDiagnostics::NoDurableRoot)?
            .as_str()
            .to_owned();
        let paths = Paths::of(s, root, self.source_root.as_deref());
        let root_identity = RootIdentity {
            module,
            compiler: COMPILER.to_owned(),
        };
        let snapshot = match self.semantic_snapshot(root, version) {
            crate::SnapshotResult::Available(snap) => Some(digest(
                crate::SnapshotResult::Available(snap).to_json().as_bytes(),
            )),
            crate::SnapshotResult::Unavailable(_) => None,
        };
        let state = State {
            source: source_digest(s, &paths, &root_identity),
            snapshot,
        };
        let complete = !incomplete(s);
        let mut outlines: BTreeMap<FileId, Vec<crate::symbols::Symbol>> = BTreeMap::new();
        let span = |at: Span| (at != Span::synthetic()).then(|| paths.span(at)).flatten();

        // The compiler's order, with each diagnostic's ordinal among those with its code
        // and primary span.
        let mut keyed = Vec::with_capacity(s.analysis.diagnostics.len());
        for (k, d) in s.analysis.diagnostics.iter().enumerate() {
            let compiler_ordinal = s
                .analysis
                .diagnostics
                .iter()
                .take(k)
                .filter(|e| e.code == d.code && e.primary.span == d.primary.span)
                .count();
            let primary = span(d.primary.span);
            let secondary: Vec<Option<SourceSpan>> =
                d.secondary.iter().map(|l| span(l.span)).collect();
            let fixes: Vec<Option<SourceSpan>> = d.fixes.iter().map(|f| span(f.span)).collect();
            let owner = if complete && d.primary.span != Span::synthetic() {
                let file = d.primary.span.file;
                let symbols =
                    outlines
                        .entry(file)
                        .or_insert_with(|| match s.files.get(file.index()) {
                            Some(FileKey::Path(_)) => declared_in(s, file),
                            _ => Vec::new(),
                        });
                owner_of(s, symbols, d.primary.span)
            } else {
                None
            };
            keyed.push((primary, secondary, fixes, k, d, owner, compiler_ordinal));
        }
        keyed.sort_by(|a, b| {
            (structure(&a.0, a.4.code.as_str(), &a.1, &a.2), a.3)
                .cmp(&(structure(&b.0, b.4.code.as_str(), &b.1, &b.2), b.3))
        });
        let mut entries: Vec<Entry<'_>> = Vec::with_capacity(keyed.len());
        for (primary, secondary, fixes, _, d, owner, compiler_ordinal) in keyed {
            let place = primary.as_ref().map_or_else(
                || "-".to_owned(),
                |p| format!("{}:{}-{}", p.file, p.start, p.end),
            );
            let stem = format!("{}@{place}", d.code);
            let n = entries
                .iter()
                .filter(|e| e.id.rsplit_once('#').map(|(s, _)| s) == Some(stem.as_str()))
                .count();
            entries.push(Entry {
                id: format!("{stem}#{n}"),
                d,
                primary,
                secondary,
                fixes,
                owner,
                compiler_ordinal,
            });
        }
        Ok(Current {
            s,
            paths,
            root: root_identity,
            state,
            entries,
        })
    }
}

/// The durable function, record or enum whose declaration contains `at`.
fn owner_of(s: &Snapshot, symbols: &[crate::symbols::Symbol], at: Span) -> Option<Owner> {
    let sym = symbols
        .iter()
        .find(|sym| sym.start <= at.start && at.end <= sym.end)?;
    let kind = match sym.kind {
        SymbolKind::Function => "function",
        SymbolKind::Record => "record",
        SymbolKind::Enum => "enum",
        _ => return None,
    };
    Some(Owner {
        identity: identity_of(s, sym.target).definition?,
        kind,
        name: sym.name.clone(),
    })
}

/// BLAKE3 over the root's identity and every loaded source file — root-relative path and
/// exact bytes — in load order, length-prefixed so no two sequences share an encoding. The
/// core prelude is the compiler's, and the compiler is in the identity.
fn source_digest(s: &Snapshot, paths: &Paths, root: &RootIdentity) -> String {
    let mut h = blake3::Hasher::new();
    let mut field = |bytes: &[u8]| {
        h.update(bytes.len().to_string().as_bytes());
        h.update(b":");
        h.update(bytes);
    };
    field(INDEX_SCHEMA.as_bytes());
    field(b"source");
    field(root.module.as_bytes());
    field(root.compiler.as_bytes());
    for (i, k) in s.files.iter().enumerate() {
        if !matches!(k, FileKey::Path(_)) {
            continue;
        }
        let path = paths.file(FileId::from_index(i)).unwrap_or_default();
        field(path.as_bytes());
        field(s.texts.get(i).map_or(&b""[..], |t| t.as_bytes()));
    }
    h.finalize().to_hex().to_string()
}

pub(crate) fn fix_counts(d: &Diagnostic) -> FixCounts {
    let automatic = d
        .fixes
        .iter()
        .filter(|f| f.applicability == Applicability::Automatic)
        .count();
    let needs_review = d
        .fixes
        .iter()
        .filter(|f| f.applicability == Applicability::NeedsReview)
        .count();
    FixCounts {
        total: d.fixes.len(),
        automatic,
        needs_review,
    }
}

pub(crate) const fn severity(d: &Diagnostic) -> &'static str {
    match d.severity {
        nazm_diag::Severity::Error => "error",
    }
}

const fn applicability(a: Applicability) -> &'static str {
    match a {
        Applicability::Automatic => "automatic",
        Applicability::NeedsReview => "needs_review",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                .spawn_scoped(scope, f)
                .expect("spawn")
                .join()
                .expect("join")
        })
    }

    const MAIN: &str = "fn main() -> Int { 1 + true }\n";

    /// A service with `MAIN` open and `extra` appended to its analysis's diagnostics.
    fn with(extra: Vec<Diagnostic>) -> (LanguageService, std::path::PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.nz");
        std::fs::write(&main, MAIN).unwrap();
        let mut s = LanguageService::new();
        s.open(&main, 0, MAIN.to_owned());
        let snapshot = s
            .documents
            .get_mut(&identity(&main))
            .and_then(|d| d.analysis.as_mut())
            .unwrap();
        snapshot.analysis.diagnostics.extend(extra);
        (s, main, dir)
    }

    #[test]
    fn two_diagnostics_structure_cannot_tell_apart_are_told_apart_by_an_ordinal() {
        on_the_service_stack(|| {
            let at = Span::new(FileId::ROOT, 3, 7);
            let code = nazm_diag::codes::TYPE_MISMATCH;
            let (mut s, main, _dir) = with(vec![
                Diagnostic::error(code.clone(), at, "first"),
                Diagnostic::error(code.clone(), at, "second"),
            ]);
            let i: serde_json::Value =
                serde_json::from_str(&s.diagnostic_index(&main, 0).to_json()).unwrap();
            let ids: Vec<&str> = i["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["id"].as_str().unwrap())
                .collect();
            assert!(ids.contains(&"N0300@main.nz:3-7#0"), "{ids:?}");
            assert!(ids.contains(&"N0300@main.nz:3-7#1"), "{ids:?}");
            let state = i["state"]["source"].as_str().unwrap().to_owned();
            // Each id is its own diagnostic, in the compiler's order.
            for (id, message) in [
                ("N0300@main.nz:3-7#0", "first"),
                ("N0300@main.nz:3-7#1", "second"),
            ] {
                let v: serde_json::Value =
                    serde_json::from_str(&s.diagnostic_detail(&main, 0, id, &state).to_json())
                        .unwrap();
                assert_eq!(v["diagnostic"]["message"], message);
            }
        });
    }

    #[test]
    fn prose_is_no_part_of_an_index() {
        on_the_service_stack(|| {
            let at = Span::new(FileId::ROOT, 3, 7);
            let code = nazm_diag::codes::TYPE_MISMATCH;
            let secondary = Span::new(FileId::ROOT, 9, 10);
            let plain = Diagnostic::error(code.clone(), at, "a").secondary(secondary, "b");
            let worded = Diagnostic::error(code.clone(), at, "an entirely different sentence")
                .primary_label("another")
                .secondary(secondary, "and another")
                .help("with help");
            let (a, main_a, _da) = with(vec![plain]);
            let (b, main_b, _db) = with(vec![worded]);
            let strip = |j: String| {
                let mut v: serde_json::Value = serde_json::from_str(&j).unwrap();
                v["state"] = serde_json::Value::Null;
                v
            };
            assert_eq!(
                strip(a.diagnostic_index(&main_a, 0).to_json()),
                strip(b.diagnostic_index(&main_b, 0).to_json())
            );
        });
    }

    /// The structural half of "never from prose": this module never reads a diagnostic's
    /// message, help, label text, fix description or precondition to decide anything — it
    /// forwards them to a detail through the one published form, and that is all — and it
    /// keeps nothing between requests.
    #[test]
    fn a_compact_diagnostic_is_never_read_from_prose() {
        let source = include_str!("diagnostics.rs");
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
            ".message",
            ".help",
            ".description",
            ".precondition",
            "Regex",
            "regex",
            ".find(\"",
            ".contains(\"",
            "starts_with(\"",
            "split_whitespace",
            "HashMap",
            "OnceLock",
            "thread_local",
            "static mut",
            "fs::",
            "read_dir",
            "SystemTime",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in diagnostics: a compact fact would come from prose or from outside the request"
            );
        }
    }
}
