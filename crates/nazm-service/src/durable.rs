//! A root compilation's durable definitions with the range of each one's name, and the files
//! the compilation loaded. N33.
//!
//! # The contract
//!
//! **Exactly the definitions the semantic snapshot tracks (N28), and nothing it does not.**
//! The list is derived from [`LanguageService::semantic_snapshot`]'s own manifest — the same
//! durable keys, the same exclusions: a definition with no durable key, or whose key the
//! compilation gives to two declarations, is not here and is counted as `untracked`. What
//! this adds is where each one's name is, as the outline (N22) has it, so a caller can ask
//! for the definition's context packet (N27) at that name without searching for a spelling.
//! A compilation that needed syntax recovery, or could not load a module, has none — the
//! snapshot's own refusal. The files are the ones the compilation loaded, never scanned.

use std::path::{Path, PathBuf};

use nazm_span::FileId;

use crate::context::Paths;
use crate::service::{FileKey, LanguageService};
use crate::snapshot::NoSnapshot;
use crate::symbols::declared_in;

/// One durable definition, and where its declaration and its name are.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DurableDefinition {
    /// Its durable definition key, `module::kind name`.
    pub identity: String,
    /// `function`, `record` or `enum`.
    pub kind: &'static str,
    /// Relative to the compilation's source root.
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub name_start: u32,
    pub name_end: u32,
}

/// A root compilation's durable definitions, ordered by identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDefinitions {
    /// The root module's durable key.
    pub module: String,
    pub definitions: Vec<DurableDefinition>,
    /// As the snapshot counts them.
    pub untracked: usize,
}

impl LanguageService {
    /// The durable definitions of the compilation rooted at the open document `root`, at
    /// `version`, each with its name's range — or the snapshot's reason there are none.
    ///
    /// # Errors
    ///
    /// Exactly when [`LanguageService::semantic_snapshot`] has no snapshot, for the same
    /// reason.
    pub fn durable_definitions(
        &self,
        root: &Path,
        version: i32,
    ) -> Result<DurableDefinitions, NoSnapshot> {
        let snapshot = self.derive_snapshot(root, version)?;
        let s = self.current(root, version).ok_or(NoSnapshot::NotCurrent)?;
        let paths = Paths::of(s, root, self.source_root.as_deref());
        // Every declaration's name, by its file and its whole range: top-level declarations
        // do not overlap, so the range is the declaration.
        let mut names = std::collections::BTreeMap::new();
        for (i, k) in s.files.iter().enumerate() {
            if !matches!(k, FileKey::Path(_)) {
                continue;
            }
            let file = FileId::from_index(i);
            let Some(relative) = paths.file(file) else {
                continue;
            };
            for sym in declared_in(s, file) {
                names.insert(
                    (relative.clone(), sym.start, sym.end),
                    (sym.name_start, sym.name_end),
                );
            }
        }
        let mut definitions = Vec::with_capacity(snapshot.definitions.len());
        let mut untracked = snapshot.untracked;
        for d in snapshot.definitions {
            let at = (
                d.declaration.file.clone(),
                d.declaration.start,
                d.declaration.end,
            );
            match names.get(&at) {
                Some(&(name_start, name_end)) => definitions.push(DurableDefinition {
                    identity: d.identity,
                    kind: d.kind,
                    file: d.declaration.file,
                    start: d.declaration.start,
                    end: d.declaration.end,
                    name_start,
                    name_end,
                }),
                None => untracked += 1,
            }
        }
        Ok(DurableDefinitions {
            module: snapshot.root.module,
            definitions,
            untracked,
        })
    }

    /// The files the compilation rooted at the open document `root` loaded, at `version`, by
    /// the path each was read from — the core prelude, which has none, left out. `None` when
    /// `root` is not open at that version.
    #[must_use]
    pub fn compilation_files(&self, root: &Path, version: i32) -> Option<Vec<PathBuf>> {
        let s = self.current(root, version)?;
        Some(
            s.files
                .iter()
                .filter_map(|k| match k {
                    FileKey::Path(p) => Some(p.clone()),
                    FileKey::Prelude => None,
                })
                .collect(),
        )
    }
}
