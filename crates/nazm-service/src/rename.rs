//! Safe semantic rename: a validated edit plan, never a text search. N18.
//!
//! # The rule
//!
//! A rename is offered only when every source occurrence that must change is **known** —
//! recorded by the checker as the entity being renamed — and only when the program it
//! leaves behind is proved to mean what the old one meant. Nothing here searches for a
//! spelling: the edits are the entity's declaration and its uses in the reference index,
//! and a rename that the index cannot make complete is refused, never completed from text.
//!
//! ```text
//! position → token (CST) → entity (index, from Resolution) → coverage → scope
//!          → edits (declaration + uses) → candidate program → re-parse, re-check
//!          → same partition of occurrences into entities → RenamePlan
//! ```
//!
//! # Which renames are safe, and why
//!
//! A plan's occurrence universe must be provably complete. Two scopes are, from the
//! language's own rules, and nothing wider is claimed:
//!
//! - **A local** — a parameter, `let` or pattern binding — is written only inside its own
//!   function's body, which the analysis resolved in full.
//! - **A private top-level entity** — a function, record or enum without `pub`, and the
//!   fields, variants and payload fields of a private record or enum — can be named only by
//!   its own module: resolution binds a name in another module only to an import's
//!   exports, and a public item that would expose a private type is refused (`N0337`), so a
//!   value of a private type never reaches another module. Its occurrences are all in its
//!   declaring file, which the analysis resolved in full.
//!
//! An **exported** entity is refused. Any file under any source root may import its module,
//! and the service knows only the programs rooted at open documents; a directory an editor
//! opened is not a project and there is no manifest that would make one. Completeness for
//! files nobody opened cannot be proved, so it is not claimed.
//!
//! A **core-prelude** entity is refused: the toolchain owns it.
//!
//! # What "the same meaning" is checked as
//!
//! The candidate program — the analysed buffers with the edits applied in memory, nothing
//! written — is loaded and checked by the same loader and checker as every other analysis,
//! once for every open compilation that contains the edited file. It must have no
//! diagnostics, as the program before it had none (a rename is offered on a clean program
//! only). And its occurrences must partition into entities exactly as the old program's
//! did, once the old offsets are moved past the edits: the same groups of occurrences, of
//! the same kinds. So an edit cannot capture a use into another binding, merge two
//! entities, split one, or lose a resolution, without the rename being refused.
//! Session-local ids are not compared — they are not durable — and a renamed top-level
//! entity's durable key and its module's interface fingerprint *do* change, which is what
//! renaming it means.
//!
//! # Stale-safe
//!
//! A plan records, for each file it edits, the exact text it was computed from and the
//! document version that text is, and each edit the bytes it expects to replace. It is
//! current only while the service's generation, every such version and every such text
//! are unchanged ([`LanguageService::plan_is_current`]). The service never applies a plan:
//! the editor does, and what the protocol can hold it to is the adapter's question.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nazm_core::__syntax::TokenKind;
use nazm_core::sema::{Coverage, ReferenceTarget};
use nazm_core::Resolution;
use nazm_diag::Diagnostic;
use nazm_span::{FileId, Span};

use crate::load::{Disk, Sources};
use crate::service::{analyse, identity, name_at, FileKey, LanguageService, Overlaid, Snapshot};
use crate::Target;

/// One replacement: the bytes `start..end` of a file, which must read `expected`, become
/// `replacement`. The one edit every plan is made of — a rename's (N18), a fix's (N24) and a
/// patch's (N29), which serialises it as it is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub expected: String,
    pub replacement: String,
}

/// Every edit to one file, and the exact source they are edits of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    /// The open document's version the plan was computed from, or `None` for a file that
    /// is not open — whose only precondition is then its text.
    pub version: Option<i32>,
    /// The file's exact text at planning: the precondition every offset is relative to.
    pub text: Arc<str>,
    /// Ordered by start, disjoint, each once.
    pub edits: Vec<Edit>,
}

/// A validated rename: the complete set of edits, and what they are valid against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    /// `function`, `local`, `record`, `enum`, `variant`, `field` or `payload field`.
    pub kind: &'static str,
    pub old_name: String,
    pub new_name: String,
    /// The service generation the plan was computed and validated at.
    pub generation: u64,
    /// Ordered by path; one entry per file.
    pub files: Vec<FileEdit>,
}

/// What `prepare_rename` found: the name under the position, which a rename would change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renameable {
    pub kind: &'static str,
    pub name: String,
    /// The token the position is on, in the document's text.
    pub start: u32,
    pub end: u32,
}

/// Why a rename is not offered. Every refusal is a whole refusal: there is no partial plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The document is not open at that version.
    NotCurrent,
    /// The position is on no name the resolution identified as an entity: whitespace, a
    /// comment, a literal, a keyword, a built-in, a type parameter, a name that did not
    /// resolve.
    NoEntity,
    /// The entity's occurrences are not all recorded.
    Incomplete(&'static str),
    /// The core prelude's: owned by the toolchain.
    ToolchainOwned,
    /// Exported, with no workspace to find every use in: modules no open program loads may
    /// use it.
    Exported,
    /// Exported from a library package (N89): other packages import its API, and no walk of
    /// this workspace finds them.
    PackageApi,
    /// A compilation containing the entity has diagnostics, so its resolution is not a
    /// complete account of the program.
    NotClean,
    /// The new name is not an identifier, or is one the language gives another meaning.
    InvalidName(&'static str),
    /// The new name is the old one.
    Unchanged,
    /// The candidate program does not check.
    Rejected(Vec<Diagnostic>),
    /// The candidate program checks and means something else: its occurrences do not
    /// group into entities as the old program's did.
    MeaningChanged,
    /// An invariant of the plan did not hold. Refused rather than repaired.
    Internal(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCurrent => f.write_str("the document is not at the version asked about"),
            Self::NoEntity => f.write_str("nothing a program declared is named here"),
            Self::Incomplete(why) => write!(f, "not every occurrence is known: {why}"),
            Self::ToolchainOwned => {
                f.write_str("this is the core prelude's, which the toolchain owns")
            }
            Self::Exported => f.write_str(
                "this is exported, and a module no open program loads may use it, so every \
                 occurrence cannot be known; declare a workspace (a `nazm.root` marker or \
                 `--source-root`) and every module under it is searched",
            ),
            Self::PackageApi => f.write_str(
                "this is part of a library package's API, which other packages import; no walk \
                 of this workspace finds their uses, so it is not renamed",
            ),
            Self::NotClean => {
                f.write_str("a program containing this has errors, so its uses are not all known")
            }
            Self::InvalidName(why) => write!(f, "the new name {why}"),
            Self::Unchanged => f.write_str("the new name is the current one"),
            Self::Rejected(d) => write!(
                f,
                "the renamed program would not check: {}",
                d.first().map_or("", |d| d.message.as_str())
            ),
            Self::MeaningChanged => f.write_str(
                "the renamed program would mean something else: a use would refer to a \
                 different definition",
            ),
            Self::Internal(why) => write!(f, "refused: {why}"),
        }
    }
}

impl LanguageService {
    /// Whether the name at byte `offset` of `path`, at `version`, can be renamed, and if so
    /// which token and what it is called now. Every check but the new name's is made here.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] that applies.
    pub fn prepare_rename(
        &self,
        path: &Path,
        version: i32,
        offset: u32,
    ) -> Result<Renameable, Refusal> {
        self.renameable(path, version, offset).map(|(r, _)| r)
    }

    /// The validated plan that renames the entity at byte `offset` of `path`, at `version`,
    /// to `new_name`. Nothing is written.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] that applies. A refusal is never accompanied by a partial plan.
    pub fn rename(
        &self,
        path: &Path,
        version: i32,
        offset: u32,
        new_name: &str,
    ) -> Result<RenamePlan, Refusal> {
        let (found, (entity, declared)) = self.renameable(path, version, offset)?;
        valid_name(new_name)?;
        if new_name == found.name {
            return Err(Refusal::Unchanged);
        }
        let s = self.current(path, version).ok_or(Refusal::NotCurrent)?;
        if exported(&s.analysis.resolution, entity) {
            let root = self.workspace(path)?.ok_or(Refusal::Exported)?;
            return self.workspace_plan(&root, s, &found, declared, new_name);
        }

        // The edits: the declaration and every use, and nothing else. All in the declaring
        // file — that is what the scope rule above proved, and it is checked, not assumed.
        let mut spans = vec![declared];
        spans.extend_from_slice(s.index.uses(entity));
        spans.sort_unstable();
        if spans.iter().any(|u| u.file != declared.file) {
            return Err(Refusal::Internal("a use outside the declaring file"));
        }
        let text = s
            .texts
            .get(declared.file.index())
            .ok_or(Refusal::Internal("the declaring file has no text"))?
            .clone();
        let mut edits: Vec<Edit> = Vec::with_capacity(spans.len());
        for span in &spans {
            let expected = text
                .get(span.start as usize..span.end as usize)
                .ok_or(Refusal::Internal("an occurrence outside its file"))?;
            if expected != found.name {
                return Err(Refusal::Internal(
                    "an occurrence is not spelled as its entity",
                ));
            }
            if edits.last().is_some_and(|e| e.end > span.start) {
                return Err(Refusal::Internal("two occurrences overlap"));
            }
            edits.push(Edit {
                start: span.start,
                end: span.end,
                expected: expected.to_owned(),
                replacement: new_name.to_owned(),
            });
        }
        let edited = apply(&text, &edits);

        let Some(Target::File(at)) = s.target(declared) else {
            return Err(Refusal::ToolchainOwned);
        };
        let key = s
            .files
            .get(declared.file.index())
            .cloned()
            .ok_or(Refusal::Internal("the declaring file is unknown"))?;
        let FileKey::Path(identity_path) = &key else {
            return Err(Refusal::ToolchainOwned);
        };
        self.validate(&key, identity_path, &text, &edited, &edits)?;

        let version = self.documents.get(identity_path).map(|d| d.version);
        Ok(RenamePlan {
            kind: found.kind,
            old_name: found.name,
            new_name: new_name.to_owned(),
            generation: self.generation,
            files: vec![FileEdit {
                path: at.path,
                version,
                text,
                edits,
            }],
        })
    }

    /// Whether `plan` still describes the service's current program: the same generation,
    /// and every file it edits at the version and with the exact text it was computed
    /// from. A plan that is not current must not be applied.
    #[must_use]
    pub fn plan_is_current(&self, plan: &RenamePlan) -> bool {
        plan.generation == self.generation && plan.files.iter().all(|f| self.file_is_current(f))
    }

    /// Whether one file's edits still describe that file: it is at the version the edits
    /// were computed against (or, for a file that is not open, on disk) with exactly the
    /// text they were computed from, and every edit's bytes are the ones it expects to
    /// replace. The one freshness law every edit plan answers to — a rename's (N18) and a
    /// fix's (N24).
    pub(crate) fn file_is_current(&self, f: &FileEdit) -> bool {
        let key = identity(&f.path);
        let text_now: Option<Arc<str>> = match self.documents.get(&key) {
            Some(d) if f.version == Some(d.version) => Some(d.text.clone()),
            Some(_) => None,
            None if f.version.is_none() => Disk.read(&f.path).ok().map(Arc::from),
            None => None,
        };
        text_now.is_some_and(|t| *t == *f.text)
            && f.edits
                .iter()
                .all(|e| f.text.get(e.start as usize..e.end as usize) == Some(e.expected.as_str()))
    }

    /// The entity at a position, if every rule but the new name's allows renaming it: with
    /// the entity and where it is declared.
    pub(crate) fn renameable(
        &self,
        path: &Path,
        version: i32,
        offset: u32,
    ) -> Result<(Renameable, (ReferenceTarget, Span)), Refusal> {
        let s = self.current(path, version).ok_or(Refusal::NotCurrent)?;
        let at = name_at(&s.cst, offset).ok_or(Refusal::NoEntity)?;
        let entity = s.index.target(at).ok_or(Refusal::NoEntity)?;
        if let Coverage::Incomplete(why) = entity.rename_coverage() {
            return Err(Refusal::Incomplete(why));
        }
        let declared = s.index.declaration(entity).ok_or(Refusal::ToolchainOwned)?;
        let key = s
            .files
            .get(declared.file.index())
            .ok_or(Refusal::Internal("the declaring file is unknown"))?;
        if *key == FileKey::Prelude {
            return Err(Refusal::ToolchainOwned);
        }
        if exported(&s.analysis.resolution, entity) {
            // Within a declared workspace every module that can name it is found (N89).
            self.workspace(path)?.ok_or(Refusal::Exported)?;
        }
        let text = s
            .texts
            .get(declared.file.index())
            .ok_or(Refusal::Internal("the declaring file has no text"))?;
        // Every compilation that contains the declaring file must be clean: a resolution
        // with errors in it is not a complete account of where anything is used.
        for other in self.containing(key, text) {
            if !other.0.analysis.diagnostics.is_empty() {
                return Err(Refusal::NotClean);
            }
        }
        let name = text
            .get(declared.start as usize..declared.end as usize)
            .ok_or(Refusal::Internal("the declaration is outside its file"))?
            .to_owned();
        Ok((
            Renameable {
                kind: entity.kind(),
                name,
                start: at.start,
                end: at.end,
            },
            (entity, declared),
        ))
    }

    /// Every current compilation that contains `file` with exactly `text`, with the
    /// [`FileId`] it has there and the document the compilation is rooted at.
    fn containing<'a>(
        &'a self,
        file: &'a FileKey,
        text: &'a Arc<str>,
    ) -> impl Iterator<Item = (&'a Snapshot, FileId, &'a Path, i32)> + 'a {
        self.documents.values().filter_map(move |d| {
            let s = d.analysis.as_ref()?;
            if s.generation != self.generation {
                return None;
            }
            let f = s
                .files
                .iter()
                .zip(&s.texts)
                .position(|(k, t)| k == file && **t == **text)?;
            Some((s, FileId::from_index(f), d.path.as_path(), d.version))
        })
    }

    /// The candidate program, checked: for every open compilation containing the edited
    /// file, the same loader and checker over the buffers with `edited` in the file's place.
    fn validate(
        &self,
        key: &FileKey,
        identity_path: &Path,
        text: &Arc<str>,
        edited: &str,
        edits: &[Edit],
    ) -> Result<(), Refusal> {
        let mut validated = 0;
        for (before, file, root, version) in self.containing(key, text) {
            let sources = Overlaid {
                documents: &self.documents,
                replaced: Some((identity_path, edited)),
                many: &[],
            };
            let after = analyse(
                &sources,
                self.source_root.as_deref(),
                root,
                version,
                self.generation,
                // A rename's trial parses the edited text in full: no revision is of it.
                None,
            );
            if !after.analysis.diagnostics.is_empty() {
                return Err(Refusal::Rejected(after.analysis.diagnostics.clone()));
            }
            if partition(before, Some((file, edits))) != partition(&after, None) {
                return Err(Refusal::MeaningChanged);
            }
            validated += 1;
        }
        if validated == 0 {
            return Err(Refusal::Internal("no compilation contains the edited file"));
        }
        Ok(())
    }
}

impl LanguageService {
    /// The workspace a document is in (N89, §7.90): its source root when that is declared — by
    /// `--source-root`, a `nazm.root` marker or a package's manifest — and `None` when the root
    /// is merely the file's directory. A library package — a manifest with no `main` — is
    /// [`Refusal::PackageApi`]: its exported API is other packages' to use.
    pub(crate) fn workspace(&self, path: &Path) -> Result<Option<PathBuf>, Refusal> {
        let Ok(root) = crate::root::decide(path, self.source_root.as_deref()) else {
            return Ok(None);
        };
        match root.how() {
            crate::root::How::Given | crate::root::How::Marker => {
                Ok(Some(root.path().to_path_buf()))
            }
            crate::root::How::Package => {
                if library_package(root.path()) {
                    Err(Refusal::PackageApi)
                } else {
                    Ok(Some(root.path().to_path_buf()))
                }
            }
            _ => Ok(None),
        }
    }

    /// Every `.nz` file under the workspace, analysed as a root over the open buffers, in path
    /// order (N89). Hidden directories and `target/` are not walked, as no other walk walks
    /// them.
    fn workspace_compilations(&self, root: &Path, many: &[(PathBuf, String)]) -> Vec<Snapshot> {
        let mut files = Vec::new();
        walk(root, &mut files);
        files.sort();
        let sources = Overlaid {
            documents: &self.documents,
            replaced: None,
            many,
        };
        files
            .iter()
            .map(|f| {
                let version = self.documents.get(&identity(f)).map_or(0, |d| d.version);
                analyse(
                    &sources,
                    self.source_root.as_deref(),
                    f,
                    version,
                    self.generation,
                    None,
                )
            })
            .collect()
    }

    /// The plan renaming an exported entity across a workspace (N89, §7.90): found in every
    /// compilation by its declaration's place, its occurrences across every file merged, the
    /// candidate workspace re-analysed whole and required to mean what it meant.
    fn workspace_plan(
        &self,
        root: &Path,
        here: &Snapshot,
        found: &Renameable,
        declared: Span,
        new_name: &str,
    ) -> Result<RenamePlan, Refusal> {
        let key = here
            .files
            .get(declared.file.index())
            .cloned()
            .ok_or(Refusal::Internal("the declaring file is unknown"))?;
        let FileKey::Path(declaring) = &key else {
            return Err(Refusal::ToolchainOwned);
        };
        if !declaring.starts_with(root) {
            return Err(Refusal::Exported);
        }
        let before = self.workspace_compilations(root, &[]);
        // Each file's text and the occurrences to change in it, from every compilation that
        // contains the declaring file.
        let mut per_file: std::collections::BTreeMap<PathBuf, Occurrences> =
            std::collections::BTreeMap::new();
        let mut holding = 0usize;
        for s in &before {
            let Some(f) = s.files.iter().position(|k| *k == key) else {
                continue;
            };
            if !s.analysis.diagnostics.is_empty() {
                return Err(Refusal::NotClean);
            }
            let at = Span::new(FileId::from_index(f), declared.start, declared.end);
            let entity = s.index.target(at).ok_or(Refusal::Internal(
                "the declaration is no entity in a compilation that contains it",
            ))?;
            holding += 1;
            let decl = s.index.declaration(entity).into_iter();
            for span in decl.chain(s.index.uses(entity).iter().copied()) {
                let FileKey::Path(p) = s
                    .files
                    .get(span.file.index())
                    .cloned()
                    .unwrap_or(FileKey::Prelude)
                else {
                    return Err(Refusal::ToolchainOwned);
                };
                let text = s
                    .texts
                    .get(span.file.index())
                    .cloned()
                    .ok_or(Refusal::Internal("a file with no text"))?;
                let entry = per_file
                    .entry(p)
                    .or_insert_with(|| (text.clone(), Default::default()));
                if *entry.0 != *text {
                    return Err(Refusal::NotCurrent);
                }
                entry.1.insert((span.start, span.end));
            }
        }
        if holding == 0 {
            return Err(Refusal::Internal(
                "no compilation of the workspace contains it",
            ));
        }
        let mut files = Vec::new();
        let mut many: Vec<(PathBuf, String)> = Vec::new();
        let mut shifts: Vec<(PathBuf, Vec<Edit>)> = Vec::new();
        for (path, (text, spans)) in &per_file {
            let mut edits: Vec<Edit> = Vec::with_capacity(spans.len());
            for (start, end) in spans {
                let expected = text
                    .get(*start as usize..*end as usize)
                    .ok_or(Refusal::Internal("an occurrence outside its file"))?;
                if expected != found.name {
                    return Err(Refusal::Internal(
                        "an occurrence is not spelled as its entity",
                    ));
                }
                if edits.last().is_some_and(|e| e.end > *start) {
                    return Err(Refusal::Internal("two occurrences overlap"));
                }
                edits.push(Edit {
                    start: *start,
                    end: *end,
                    expected: expected.to_owned(),
                    replacement: new_name.to_owned(),
                });
            }
            many.push((path.clone(), apply(text, &edits)));
            shifts.push((path.clone(), edits.clone()));
            files.push(FileEdit {
                path: path.clone(),
                version: self.documents.get(path).map(|d| d.version),
                text: text.clone(),
                edits,
            });
        }
        // The candidate workspace, every compilation of it: clean, and the same partition.
        let after = self.workspace_compilations(root, &many);
        for (b, a) in before.iter().zip(&after) {
            if !b
                .files
                .iter()
                .any(|k| matches!(k, FileKey::Path(p) if per_file.contains_key(p)))
            {
                continue;
            }
            if !a.analysis.diagnostics.is_empty() {
                return Err(Refusal::Rejected(a.analysis.diagnostics.clone()));
            }
            if partition_shifted(b, &shifts) != partition_shifted(a, &[]) {
                return Err(Refusal::MeaningChanged);
            }
        }
        Ok(RenamePlan {
            kind: found.kind,
            old_name: found.name.clone(),
            new_name: new_name.to_owned(),
            generation: self.generation,
            files,
        })
    }
}

/// One file's text and the byte ranges in it a workspace rename changes (N89).
type Occurrences = (Arc<str>, std::collections::BTreeSet<(u32, u32)>);

/// Whether the package whose source root is `root` is a library: its manifest names no `main`.
fn library_package(root: &Path) -> bool {
    root.ancestors().any(|dir| {
        let Ok(text) = std::fs::read_to_string(dir.join(nazm_package::MANIFEST)) else {
            return false;
        };
        nazm_package::Manifest::parse(&text, "nazm.toml")
            .is_ok_and(|m| dir.join(&m.root) == root && m.main.is_none())
    })
}

/// Every `.nz` file under `dir`, skipping hidden directories and `target/`.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !name.starts_with('.') && name != "target" {
                walk(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "nz") {
            out.push(p);
        }
    }
}

/// [`partition`] with several files' offsets moved past their edits (N89).
fn partition_shifted(
    s: &Snapshot,
    shifts: &[(PathBuf, Vec<Edit>)],
) -> std::collections::BTreeSet<(&'static str, Vec<Place>)> {
    let place = |span: Span| -> Option<Place> {
        let key = s.files.get(span.file.index())?.clone();
        let edits = match &key {
            FileKey::Path(p) => shifts
                .iter()
                .filter(|(q, _)| q == p)
                .map(|(_, e)| e.as_slice())
                .next(),
            FileKey::Prelude => None,
        };
        let (start, end) = edits.map_or((span.start, span.end), |e| moved(span, e));
        Some((key, start, end))
    };
    s.index
        .iter()
        .map(|(t, uses)| {
            let mut places: Vec<Place> = s
                .index
                .declaration(t)
                .into_iter()
                .chain(uses.iter().copied())
                .filter_map(place)
                .collect();
            places.sort();
            (t.kind(), places)
        })
        .collect()
}

/// Whether the entity's owning top-level definition is exported: a function, record or enum
/// with `pub`, or a field, variant or payload field of an exported record or enum. A local
/// is never.
fn exported(res: &Resolution, entity: ReferenceTarget) -> bool {
    let record = |id| res.records().get(id).map(|d| d.visibility.is_exported());
    let enumeration = |id| res.enums().get(id).map(|d| d.visibility.is_exported());
    let answer = match entity {
        ReferenceTarget::Local(_) => Some(false),
        ReferenceTarget::Function(id) => res.function(id).map(|f| f.visibility.is_exported()),
        ReferenceTarget::Record(id) => record(id),
        ReferenceTarget::Field(f) => record(f.record),
        ReferenceTarget::Enum(id) => enumeration(id),
        ReferenceTarget::Variant(v) => enumeration(v.enumeration),
        ReferenceTarget::PayloadField(p) => enumeration(p.enumeration),
        ReferenceTarget::Trait(t) | ReferenceTarget::TraitMethod(t, _) => {
            res.traits().get(t).map(|d| d.visibility.is_exported())
        }
    };
    // An entity the tables do not have is treated as exported: refused, not guessed.
    answer.unwrap_or(true)
}

/// Whether `name` may replace an identifier: exactly one identifier token, as the lexer
/// every compilation uses reads it — so a keyword, punctuation, a space, a leading digit or
/// an empty string is refused by the same rules that would refuse it in a file. `_` is
/// refused too: in a pattern it is not a name but a discard.
fn valid_name(name: &str) -> Result<(), Refusal> {
    if name.is_empty() {
        return Err(Refusal::InvalidName("is empty"));
    }
    if name == "_" {
        return Err(Refusal::InvalidName(
            "is `_`, which discards rather than names",
        ));
    }
    let (tokens, diagnostics) = nazm_core::__syntax::lex(name);
    if !diagnostics.is_empty() {
        return Err(Refusal::InvalidName("is not a token"));
    }
    let names: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Eof))
        .collect();
    match names.as_slice() {
        [t] if matches!(&t.kind, TokenKind::Ident(n) if n == name)
            && t.span.start == 0
            && t.span.end as usize == name.len() =>
        {
            Ok(())
        }
        [t] if !matches!(t.kind, TokenKind::Ident(_)) => Err(Refusal::InvalidName(
            "is a keyword or a symbol, not an identifier",
        )),
        _ => Err(Refusal::InvalidName("is not one identifier")),
    }
}

/// `text` with `edits` — ordered and disjoint — applied.
fn apply(text: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    for e in edits {
        out.push_str(text.get(at..e.start as usize).unwrap_or_default());
        out.push_str(&e.replacement);
        at = e.end as usize;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}

/// One occurrence, placed so that two compilations of one program can compare it: the file
/// by identity, and the byte range.
type Place = (FileKey, u32, u32);

/// How a compilation's occurrences group into entities: for each entity, its kind and the
/// sorted places of its declaration and uses. The set of those groups — ids left out, since
/// they are session-local — is what a rename must leave unchanged.
///
/// `shift` moves one file's offsets past a rename's edits, so the old program's groups are
/// stated in the new program's coordinates.
fn partition(
    s: &Snapshot,
    shift: Option<(FileId, &[Edit])>,
) -> std::collections::BTreeSet<(&'static str, Vec<Place>)> {
    let place = |span: Span| -> Option<Place> {
        let key = s.files.get(span.file.index())?.clone();
        let (start, end) = match shift {
            Some((file, edits)) if file == span.file => moved(span, edits),
            _ => (span.start, span.end),
        };
        Some((key, start, end))
    };
    s.index
        .iter()
        .map(|(t, uses)| {
            let mut places: Vec<Place> = s
                .index
                .declaration(t)
                .into_iter()
                .chain(uses.iter().copied())
                .filter_map(place)
                .collect();
            places.sort();
            (t.kind(), places)
        })
        .collect()
}

/// Where `span` is after `edits` — ordered, disjoint — are applied to its file: past every
/// edit that ends at or before it, and, for an edited span itself, its replacement's extent.
fn moved(span: Span, edits: &[Edit]) -> (u32, u32) {
    let mut delta: i64 = 0;
    for e in edits {
        let grew =
            i64::try_from(e.replacement.len()).unwrap_or(0) - i64::from(e.end) + i64::from(e.start);
        if e.start == span.start && e.end == span.end {
            let start = i64::from(span.start) + delta;
            let end = start + i64::try_from(e.replacement.len()).unwrap_or(0);
            return (
                u32::try_from(start).unwrap_or(u32::MAX),
                u32::try_from(end).unwrap_or(u32::MAX),
            );
        }
        if e.end <= span.start {
            delta += grew;
        }
    }
    (
        u32::try_from(i64::from(span.start) + delta).unwrap_or(u32::MAX),
        u32::try_from(i64::from(span.end) + delta).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_name_is_one_identifier_by_the_lexers_rules() {
        assert_eq!(valid_name("renamed"), Ok(()));
        assert_eq!(valid_name("snake_case_2"), Ok(()));
        for bad in [
            "",
            "_",
            "two words",
            "a-b",
            "a.b",
            "9lives",
            "fn",
            "let",
            "match",
            "(",
            "x;",
        ] {
            assert!(valid_name(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn offsets_move_past_the_edits_before_them() {
        let e = |start, end, r: &str| Edit {
            start,
            end,
            expected: "ab".into(),
            replacement: r.into(),
        };
        let edits = [e(2, 4, "xyz"), e(10, 12, "q")];
        let span = |s, t| Span::new(FileId::ROOT, s, t);
        assert_eq!(moved(span(0, 1), &edits), (0, 1));
        assert_eq!(
            moved(span(2, 4), &edits),
            (2, 5),
            "an edit: its replacement's extent"
        );
        assert_eq!(moved(span(6, 8), &edits), (7, 9));
        assert_eq!(moved(span(10, 12), &edits), (11, 12));
        assert_eq!(moved(span(20, 21), &edits), (20, 21));
        assert_eq!(
            apply("..ab....ab..", &[e(2, 4, "xyz"), e(8, 10, "q")]),
            "..xyz....q.."
        );
    }

    /// The structural half of "never a text search": this module computes its edits from
    /// the index alone. No search of a file for a spelling exists here to fall back to.
    #[test]
    fn a_rename_never_searches_text() {
        let source = include_str!("rename.rs");
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
            ".find(",
            ".rfind(",
            "match_indices",
            ".contains(",
            ".replace(",
            ".split(",
            "parse_source(",
            "check(",
            "HashMap<String",
            "BTreeMap<String",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in rename.rs: an edit would be found by its text"
            );
        }
    }
}
