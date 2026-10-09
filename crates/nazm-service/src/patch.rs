//! Semantic patch plans: an edit the compiler already knows how to validate, as minimal
//! exact-byte edits bound to the source and semantic state they were planned from.
//! `nazm.patch/1`. N29.
//!
//! # The contract
//!
//! **A patch is a proposal bound to exact source state, not permission to mutate files.**
//! N29 plans edits and never applies them: nothing here writes a file, and no surface built
//! on it can.
//!
//! Two operations, each read from the layer that already owns it, and nothing else:
//!
//! | Operation | From |
//! | --- | --- |
//! | `rename` | N18's validated rename plan: the entity at the position (N17), its declaration and uses, the scope rule, the new name's lexing, and the candidate program re-parsed, re-checked, clean and partitioned into entities exactly as before |
//! | `diagnostic_fix` | N24's projection of a [`Fix`](nazm_diag::Fix) a current diagnostic carries — never a code, a message or a spelling |
//!
//! N18's restrictions stand: an exported entity is refused, because one root compilation is
//! not a complete importer universe, and a snapshot of that compilation does not make it
//! one.
//!
//! # Freshness, in three layers
//!
//! | Layer | Protects |
//! | --- | --- |
//! | `baseline.snapshot` — BLAKE3 of the root's `nazm.snapshot/3` bytes (N28) | the semantic state of the whole root compilation |
//! | `files[].digest` — BLAKE3 of the file's exact bytes | the whole of each edited file |
//! | `edits[].expected` — the exact bytes each edit replaces | each edit's place and content |
//!
//! A future applier refuses a patch unless all three still hold. A snapshot can agree while
//! a file does not (a comment elsewhere in it is `source` of one definition, and no digest
//! of the edited one); a file digest says the file moved without saying where; expected
//! bytes say what each edit is of. None is implied by another, so all three are kept.
//!
//! # The disk plan
//!
//! The root is read from disk and analysed; a file of that compilation the edit is in is
//! opened in this request's service from the text the analysis read — no second read — so
//! N18 and N24 plan against it as they would against an open document. That is a
//! read-only disk plan, protected by the three layers above; it changes nothing about the
//! language server, whose rename still edits open documents only.
//!
//! # Minimal, and canonical
//!
//! A rename is one edit per occurrence, each the name's bytes; a fix is the compiler's one
//! span. No file text is carried, and no edit is widened. Files are ordered by
//! root-relative path and edits by start then end; a duplicate edit, two edits that
//! overlap, two insertions at one point, or an edit that changes nothing refuse the whole
//! patch — nothing is merged, dropped or repaired. Serialisation order is not application
//! order: an applier that works from the end of a file backwards reverses the list itself.
//! Nothing session-local is written, so the same sources and request give the same bytes.

use std::path::{Path, PathBuf};

use nazm_core::sema::ReferenceTarget;
use nazm_diag::Applicability;
use nazm_span::{FileId, Span};
use serde::Serialize;

use crate::context::{identity_of, incomplete, Identity, Paths, SourceSpan};
use crate::rename::{Edit, FileEdit, Refusal};
use crate::service::{identity, FileKey, LanguageService, Snapshot};
use crate::snapshot::RootIdentity;

/// The schema every patch declares.
pub const SCHEMA: &str = "nazm.patch/1";

/// A patch plan, or why there is none, exactly as `nazm.patch/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum PatchResult {
    Available(Box<SemanticPatch>),
    Unavailable(NoPatchResult),
}

/// A planned, validated, unapplied edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticPatch {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    pub operation: Operation,
    /// The root compilation the plan is of, as N28 identifies it.
    pub root: RootIdentity,
    pub baseline: Baseline,
    /// Ordered by path; each file once.
    pub files: Vec<PatchFile>,
    /// What established the edit: `reanalysed` for a rename (N18's candidate check), or
    /// `compiler_fix` for a fix, which is the compiler's own and is governed by its
    /// applicability and precondition.
    pub validation: &'static str,
}

/// What the patch does, by the operation that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    Rename {
        target: RenameTarget,
        old_name: String,
        new_name: String,
    },
    DiagnosticFix {
        diagnostic: FixedDiagnostic,
        /// Which of the fixes at the requested position this is.
        fix: usize,
        /// How many fixes were at that position.
        alternatives: usize,
        /// `automatic` or `needs_review`, exactly as the compiler attached it.
        applicability: &'static str,
        /// When the edit is the right one — the compiler's, present for every fix.
        precondition: String,
        /// For presentation only: never identity, selection or validation.
        description: String,
    },
}

/// The entity a rename renames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenameTarget {
    /// `function`, `local`, `record`, `enum`, `variant`, `field` or `payload_field`.
    pub kind: &'static str,
    /// Whether `identity` is a durable definition key. A local never is.
    pub durable: bool,
    /// The durable definition key and member; `None` for a local, which has none.
    pub identity: Option<Identity>,
    /// Where it is declared now — the source-linked identity every target has.
    pub declaration: SourceSpan,
    /// For a local, the durable key of the function it is declared in; `None` otherwise.
    pub within: Option<String>,
}

/// The current diagnostic a fix belongs to, by its structured facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedDiagnostic {
    pub code: String,
    /// Its primary span.
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// The whole-root freshness binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Baseline {
    /// BLAKE3, lowercase hexadecimal, of the root's `nazm.snapshot/3` canonical bytes — what
    /// `nazm snapshot --json` prints, without its newline.
    pub snapshot: String,
}

/// Every edit to one file, and the file it is of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchFile {
    /// Relative to the source root.
    pub path: String,
    /// BLAKE3, lowercase hexadecimal, of the file's exact bytes at planning.
    pub digest: String,
    /// Ordered by start, then end; disjoint.
    pub edits: Vec<Edit>,
}

/// Why there is no patch, as `nazm.patch/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoPatchResult {
    pub schema: &'static str,
    /// Always `unavailable`.
    pub status: &'static str,
    pub reason: NoPatch,
}

/// Why there is no patch. There is never a partial one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoPatch {
    /// The root is not open at that version.
    NotCurrent,
    /// The file is not one the root's compilation loaded.
    NotInCompilation,
    /// The offset is past the end of the file.
    OutOfRange,
    /// Some file needed syntax recovery, or a module could not be loaded: there is no
    /// snapshot to bind a patch to.
    IncompleteCompilation,
    /// The root module has no durable key, so there is no snapshot to bind a patch to.
    NoDurableRoot,
    /// Nothing the program declared is named at the position.
    NoEntity,
    /// Not every occurrence of the entity is recorded.
    IncompleteCoverage,
    /// The core prelude's: owned by the toolchain.
    ToolchainOwned,
    /// Exported: one root compilation is not every module that may import it.
    Exported,
    /// Part of a library package's API (N89): other packages import it.
    PackageApi,
    /// A compilation containing the entity has diagnostics.
    NotClean,
    /// The new name is not one identifier the language allows there.
    InvalidName,
    /// The new name is the current one: there is nothing to change.
    Unchanged,
    /// The renamed program would not check.
    Rejected,
    /// The renamed program would check and mean something else.
    MeaningChanged,
    /// No current diagnostic at the position has a fix with that index.
    NoSuchFix,
    /// The fix would replace bytes with the same bytes.
    EmptyEdit,
    /// Two edits are identical.
    DuplicateEdit,
    /// Two edits overlap, or insert at one point.
    OverlappingEdits,
    /// An invariant of the plan did not hold. Refused rather than repaired.
    Internal,
}

impl NoPatch {
    #[must_use]
    pub const fn result(self) -> PatchResult {
        PatchResult::Unavailable(NoPatchResult {
            schema: SCHEMA,
            status: "unavailable",
            reason: self,
        })
    }
}

impl PatchResult {
    /// The one canonical serialisation: compact JSON, fields in declaration order.
    ///
    /// # Panics
    ///
    /// Never: every field is a string, a number, a boolean, a list or an option of them.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a patch is plain data")
    }
}

impl From<Refusal> for NoPatch {
    fn from(r: Refusal) -> Self {
        match r {
            Refusal::NotCurrent => Self::NotCurrent,
            Refusal::NoEntity => Self::NoEntity,
            Refusal::Incomplete(_) => Self::IncompleteCoverage,
            Refusal::ToolchainOwned => Self::ToolchainOwned,
            Refusal::Exported => Self::Exported,
            Refusal::PackageApi => Self::PackageApi,
            Refusal::NotClean => Self::NotClean,
            Refusal::InvalidName(_) => Self::InvalidName,
            Refusal::Unchanged => Self::Unchanged,
            Refusal::Rejected(_) => Self::Rejected,
            Refusal::MeaningChanged => Self::MeaningChanged,
            Refusal::Internal(_) => Self::Internal,
        }
    }
}

/// BLAKE3 of `bytes`, as 64 lowercase hexadecimal digits: a file's digest, and — over its
/// canonical bytes — a snapshot's.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// What the request is bound to before either operation plans anything.
struct Bound {
    root: RootIdentity,
    snapshot: String,
    /// The file's path as the service opened it, and its version.
    file: PathBuf,
    version: i32,
}

impl LanguageService {
    /// The patch that renames the entity at byte `offset` of `file`, a file of the
    /// compilation rooted at the open document `root`, to `new_name`, by N18's rename.
    /// Nothing is written.
    pub fn patch_rename(
        &mut self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
        new_name: &str,
    ) -> PatchResult {
        match self.plan_rename(root, version, file, offset, new_name) {
            Ok(p) => PatchResult::Available(Box::new(p)),
            Err(why) => why.result(),
        }
    }

    /// The patch that applies fix `fix` among those N24 plans for the current diagnostics at
    /// byte `offset` of `file`, a file of the compilation rooted at the open document `root`.
    /// Nothing is written.
    pub fn patch_fix(
        &mut self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
        fix: usize,
    ) -> PatchResult {
        match self.plan_fix(root, version, file, offset, fix) {
            Ok(p) => PatchResult::Available(Box::new(p)),
            Err(why) => why.result(),
        }
    }

    fn plan_rename(
        &mut self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
        new_name: &str,
    ) -> Result<SemanticPatch, NoPatch> {
        let bound = self.bind(root, version, file, offset)?;
        let plan = self.rename(&bound.file, bound.version, offset, new_name)?;
        let (_, (entity, declared)) = self
            .renameable(&bound.file, bound.version, offset)
            .map_err(NoPatch::from)?;
        let s = self.current(root, version).ok_or(NoPatch::NotCurrent)?;
        let doc = self
            .current(&bound.file, bound.version)
            .ok_or(NoPatch::NotCurrent)?;
        let paths = Paths::of(s, root, self.source_root.as_deref());
        let target = rename_target(s, doc, &paths, entity, declared)?;
        let workspace = crate::root::decide(root, self.source_root.as_deref())
            .ok()
            .map(|r| identity(r.path()));
        let files = patch_files(s, &paths, workspace.as_deref(), &plan.files)?;
        Ok(SemanticPatch {
            schema: SCHEMA,
            status: "available",
            operation: Operation::Rename {
                target,
                old_name: plan.old_name,
                new_name: plan.new_name,
            },
            root: bound.root,
            baseline: Baseline {
                snapshot: bound.snapshot,
            },
            files,
            validation: "reanalysed",
        })
    }

    fn plan_fix(
        &mut self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
        fix: usize,
    ) -> Result<SemanticPatch, NoPatch> {
        let bound = self.bind(root, version, file, offset)?;
        let plans = self
            .fix_plans(&bound.file, bound.version, offset, offset)
            .map_err(|_| NoPatch::NotCurrent)?;
        let alternatives = plans.len();
        let plan = plans.into_iter().nth(fix).ok_or(NoPatch::NoSuchFix)?;
        if plan.file.edits.iter().all(|e| e.expected == e.replacement) {
            return Err(NoPatch::EmptyEdit);
        }
        let s = self.current(root, version).ok_or(NoPatch::NotCurrent)?;
        let paths = Paths::of(s, root, self.source_root.as_deref());
        let files = patch_files(s, &paths, None, std::slice::from_ref(&plan.file))?;
        let primary = plan.diagnostic.primary.span;
        let diagnostic = FixedDiagnostic {
            code: plan.diagnostic.code.to_string(),
            file: relative(s, &paths, &bound.file)?,
            start: primary.start,
            end: primary.end,
        };
        Ok(SemanticPatch {
            schema: SCHEMA,
            status: "available",
            operation: Operation::DiagnosticFix {
                diagnostic,
                fix,
                alternatives,
                applicability: match plan.applicability {
                    Applicability::Automatic => "automatic",
                    Applicability::NeedsReview => "needs_review",
                },
                precondition: plan.precondition,
                description: plan.description,
            },
            root: bound.root,
            baseline: Baseline {
                snapshot: bound.snapshot,
            },
            files,
            validation: "compiler_fix",
        })
    }

    /// The checks both operations share: the root is current and complete, the file is one
    /// its compilation loaded, the offset is inside it, and the root has a snapshot to bind
    /// to. The file is then open in this service, from the text the analysis read.
    fn bind(
        &mut self,
        root: &Path,
        version: i32,
        file: &Path,
        offset: u32,
    ) -> Result<Bound, NoPatch> {
        let s = self.current(root, version).ok_or(NoPatch::NotCurrent)?;
        // The file, among those the compilation loaded — by identity, never by reading it.
        let wanted = FileKey::Path(identity(file));
        let f = s
            .files
            .iter()
            .position(|k| *k == wanted)
            .ok_or(NoPatch::NotInCompilation)?;
        if incomplete(s) {
            return Err(NoPatch::IncompleteCompilation);
        }
        let text = s.texts.get(f).ok_or(NoPatch::NotInCompilation)?.clone();
        if offset as usize > text.len() {
            return Err(NoPatch::OutOfRange);
        }
        let snapshot = match self.semantic_snapshot(root, version) {
            crate::SnapshotResult::Available(snap) => snap,
            crate::SnapshotResult::Unavailable(u) => {
                return Err(match u.reason {
                    crate::NoSnapshot::NotCurrent => NoPatch::NotCurrent,
                    crate::NoSnapshot::IncompleteCompilation => NoPatch::IncompleteCompilation,
                    crate::NoSnapshot::NoDurableRoot => NoPatch::NoDurableRoot,
                })
            }
        };
        let bound_root = snapshot.root.clone();
        let snapshot_digest = digest(
            crate::SnapshotResult::Available(snapshot)
                .to_json()
                .as_bytes(),
        );
        let key = identity(file);
        let (file, version) = match self.documents.get(&key) {
            Some(d) => (d.path.clone(), d.version),
            None => {
                let _ = self.open(file, 0, text.to_string());
                (file.to_path_buf(), 0)
            }
        };
        Ok(Bound {
            root: bound_root,
            snapshot: snapshot_digest,
            file,
            version,
        })
    }
}

/// The rename's target, read from the root's compilation at the entity's declaration: its
/// durable identity where it has one, and for a local the function it is declared in.
fn rename_target(
    s: &Snapshot,
    doc: &Snapshot,
    paths: &Paths,
    entity: ReferenceTarget,
    declared: Span,
) -> Result<RenameTarget, NoPatch> {
    // The entity as the root's compilation numbers it: at the same declaration, in the same
    // file by identity. Ids are session-local; the place is not.
    let key = doc
        .files
        .get(declared.file.index())
        .ok_or(NoPatch::Internal)?;
    let f = s
        .files
        .iter()
        .position(|k| k == key)
        .map(FileId::from_index)
        .ok_or(NoPatch::Internal)?;
    let at = Span::new(f, declared.start, declared.end);
    let here = s.index.target(at).ok_or(NoPatch::Internal)?;
    if here.kind() != entity.kind() || s.index.declaration(here) != Some(at) {
        return Err(NoPatch::Internal);
    }
    let declaration = paths.span(at).ok_or(NoPatch::Internal)?;
    let kind = match here {
        ReferenceTarget::Function(_) => "function",
        ReferenceTarget::Local(_) => "local",
        ReferenceTarget::Record(_) => "record",
        ReferenceTarget::Enum(_) => "enum",
        ReferenceTarget::Variant(_) => "variant",
        ReferenceTarget::Field(_) => "field",
        ReferenceTarget::PayloadField(_) => "payload_field",
        ReferenceTarget::Trait(_) => "trait",
        ReferenceTarget::TraitMethod(..) => "trait_method",
        ReferenceTarget::Const(_) => "constant",
    };
    Ok(match here {
        ReferenceTarget::Local(l) => RenameTarget {
            kind,
            durable: false,
            identity: None,
            declaration,
            within: identity_of(s, ReferenceTarget::Function(l.function)).definition,
        },
        other => {
            let identity = identity_of(s, other);
            RenameTarget {
                kind,
                durable: identity.definition.is_some(),
                identity: Some(identity),
                declaration,
                within: None,
            }
        }
    })
}

/// A plan's file edits as patch files: each root-relative, with the digest of the text the
/// edits were computed from, which must be the text the root's compilation read. Ordered
/// and checked canonically.
fn patch_files(
    s: &Snapshot,
    paths: &Paths,
    workspace: Option<&Path>,
    planned: &[FileEdit],
) -> Result<Vec<PatchFile>, NoPatch> {
    let mut files = Vec::with_capacity(planned.len());
    for fe in planned {
        let key = FileKey::Path(identity(&fe.path));
        let path = match s.files.iter().position(|k| *k == key) {
            Some(f) => {
                if s.texts.get(f).map(|t| &**t) != Some(&*fe.text) {
                    return Err(NoPatch::NotCurrent);
                }
                paths
                    .file(FileId::from_index(f))
                    .ok_or(NoPatch::NotInCompilation)?
            }
            // A workspace rename (N89) edits files the root's compilation does not load: each
            // is named relative to the workspace root, which every one of them is under.
            None => {
                let rel = workspace
                    .and_then(|w| {
                        identity(&fe.path)
                            .strip_prefix(w)
                            .ok()
                            .map(Path::to_path_buf)
                    })
                    .ok_or(NoPatch::NotInCompilation)?;
                // `/`-separated, as every plan path is, whatever the host's separator.
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            }
        };
        for e in &fe.edits {
            if fe.text.get(e.start as usize..e.end as usize) != Some(e.expected.as_str()) {
                return Err(NoPatch::Internal);
            }
        }
        files.push(PatchFile {
            path,
            digest: digest(fe.text.as_bytes()),
            edits: fe.edits.clone(),
        });
    }
    canonical(&mut files)?;
    Ok(files)
}

/// Order files by path and edits by start then end, and refuse — never repair — a file
/// named twice, an empty file entry, a duplicate edit, or edits that overlap or insert at
/// one point.
fn canonical(files: &mut [PatchFile]) -> Result<(), NoPatch> {
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if files
        .windows(2)
        .any(|w| matches!(w, [a, b] if a.path == b.path))
    {
        return Err(NoPatch::Internal);
    }
    for f in files.iter_mut() {
        if f.edits.is_empty() {
            return Err(NoPatch::Internal);
        }
        f.edits.sort_by_key(|e| (e.start, e.end));
        for w in f.edits.windows(2) {
            let [a, b] = w else { continue };
            if a == b {
                return Err(NoPatch::DuplicateEdit);
            }
            if a.end > b.start || a.start == b.start {
                return Err(NoPatch::OverlappingEdits);
            }
        }
    }
    Ok(())
}

/// `file`'s path relative to the source root, as the root's compilation names it.
fn relative(s: &Snapshot, paths: &Paths, file: &Path) -> Result<String, NoPatch> {
    let key = FileKey::Path(identity(file));
    let f = s
        .files
        .iter()
        .position(|k| *k == key)
        .ok_or(NoPatch::NotInCompilation)?;
    paths
        .file(FileId::from_index(f))
        .ok_or(NoPatch::NotInCompilation)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn edit(start: u32, end: u32) -> Edit {
        Edit {
            start,
            end,
            expected: "ab".into(),
            replacement: "c".into(),
        }
    }

    fn file(path: &str, edits: Vec<Edit>) -> PatchFile {
        PatchFile {
            path: path.into(),
            digest: "0".repeat(64),
            edits,
        }
    }

    #[test]
    fn edits_are_ordered_and_a_duplicate_or_an_overlap_refuses_the_patch() {
        let mut files = vec![
            file("z.nz", vec![edit(9, 11), edit(1, 3)]),
            file("a.nz", vec![edit(4, 6)]),
        ];
        assert_eq!(canonical(&mut files), Ok(()));
        assert_eq!(files[0].path, "a.nz");
        assert_eq!(
            files[1].edits.iter().map(|e| e.start).collect::<Vec<_>>(),
            [1, 9]
        );
        let mut dup = vec![file("a.nz", vec![edit(1, 3), edit(1, 3)])];
        assert_eq!(canonical(&mut dup), Err(NoPatch::DuplicateEdit));
        let mut overlap = vec![file("a.nz", vec![edit(1, 4), edit(3, 5)])];
        assert_eq!(canonical(&mut overlap), Err(NoPatch::OverlappingEdits));
        let mut insertions = vec![file(
            "a.nz",
            vec![edit(2, 2), {
                let mut e = edit(2, 2);
                e.replacement = "d".into();
                e
            }],
        )];
        assert_eq!(canonical(&mut insertions), Err(NoPatch::OverlappingEdits));
        // Adjacent is not overlapping.
        let mut adjacent = vec![file("a.nz", vec![edit(1, 3), edit(3, 5)])];
        assert_eq!(canonical(&mut adjacent), Ok(()));
        let mut twice = vec![
            file("a.nz", vec![edit(1, 3)]),
            file("a.nz", vec![edit(5, 7)]),
        ];
        assert_eq!(canonical(&mut twice), Err(NoPatch::Internal));
    }

    /// The structural half of "a proposal, never a mutation": this module never writes,
    /// renames or removes a file, never searches source text for a spelling, never reads a
    /// diagnostic's code or message to decide an edit, and keeps nothing between requests.
    #[test]
    fn a_patch_is_never_applied_searched_or_invented() {
        let source = include_str!("patch.rs");
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
            "fs::",
            "Disk",
            "File::",
            "Command",
            "read_dir",
            ".find(",
            "match_indices",
            ".replace(",
            ".message",
            "codes::",
            ".help",
            "HashMap",
            "static mut",
            "OnceLock",
            "thread_local",
            "SystemTime",
            "git",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in patch: it would be applying, searching or inventing"
            );
        }
    }
}
