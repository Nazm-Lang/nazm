//! One Nazm compilation of the repository, as the compiler analyses it.
//!
//! Opened from disk through the language service exactly as `nazm check --source-root`
//! would load it, with the source root as the directory durable keys are measured from.
//! Every semantic fact is the service's: which files the compilation loaded, its durable
//! definitions (N28's, with their names), a definition's context packet (N27) and the
//! current diagnostics (N30). A compilation that loads a file outside its source root reads
//! something the repository state does not cover, and is refused.

use std::path::PathBuf;

use nazm_service::{
    ContextResult, DetailResult, DurableDefinition, DurableDefinitions, IndexResult,
    LanguageService,
};

use crate::manifest::SourceRoot;
use crate::repository::Repository;
use crate::Problem;

/// One root's compilation, open for the length of a request.
#[derive(Debug)]
pub(crate) struct Compilation {
    pub(crate) source: &'static SourceRoot,
    pub(crate) root: &'static str,
    path: PathBuf,
    dir: PathBuf,
    service: LanguageService,
    /// The module keys it loaded, in byte order.
    pub(crate) modules: Vec<String>,
    /// `None` when the compilation needed syntax recovery or could not load a module: it
    /// has no durable definitions, and none is invented for it.
    pub(crate) definitions: Option<DurableDefinitions>,
}

impl Compilation {
    pub(crate) fn open(
        repo: &Repository,
        source: &'static SourceRoot,
        root: &'static str,
    ) -> Result<Self, Problem> {
        let at = format!("{}/{root}", source.dir);
        let dir = repo
            .root
            .join(source.dir)
            .canonicalize()
            .map_err(|_| Problem::new("unreadable_root", at.clone()))?;
        let path = dir.join(root);
        let mut service = LanguageService::with_source_root(Some(dir.clone()));
        service
            .open_from_disk(&path)
            .map_err(|_| Problem::new("unreadable_root", at.clone()))?;
        let mut modules = Vec::new();
        for f in service
            .compilation_files(&path, 0)
            .ok_or_else(|| Problem::new("unreadable_root", at.clone()))?
        {
            let key = f
                .strip_prefix(&dir)
                .ok()
                .and_then(|k| k.to_str())
                .map(str::to_owned)
                .filter(|k| !k.contains('/') && !k.contains('\\'))
                .filter(|k| repo.sources.contains_key(&(source.id, k.clone())))
                .ok_or_else(|| Problem::new("source_outside_state", at.clone()))?;
            modules.push(key);
        }
        modules.sort();
        modules.dedup();
        let definitions = service.durable_definitions(&path, 0).ok();
        Ok(Self {
            source,
            root,
            path,
            dir,
            service,
            modules,
            definitions,
        })
    }

    pub(crate) fn id(&self) -> String {
        format!("root:{}:{}", self.source.id, self.root)
    }

    /// The root file, from the repository root.
    pub(crate) fn location(&self) -> String {
        format!("{}/{}", self.source.dir, self.root)
    }

    pub(crate) fn includes(&self, module: &str) -> bool {
        self.modules
            .binary_search_by(|m| m.as_str().cmp(module))
            .is_ok()
    }

    /// A durable definition of this compilation, by its key.
    pub(crate) fn definition(&self, identity: &str) -> Option<&DurableDefinition> {
        let d = self.definitions.as_ref()?;
        d.definitions
            .binary_search_by(|x| x.identity.as_str().cmp(identity))
            .ok()
            .and_then(|i| d.definitions.get(i))
    }

    /// Its context packet (N27), asked at its name.
    pub(crate) fn context(&self, d: &DurableDefinition) -> ContextResult {
        self.service
            .context(&self.path, 0, &self.dir.join(&d.file), d.name_start)
    }

    pub(crate) fn diagnostics(&self) -> IndexResult {
        self.service.diagnostic_index(&self.path, 0)
    }

    pub(crate) fn detail(&mut self, id: &str, state: &str) -> DetailResult {
        self.service.diagnostic_detail(&self.path, 0, id, state)
    }
}
