//! Which modules a compilation contains, and which of them import which.
//!
//! # A graph, not an ordered bag of files
//!
//! Until N2 the driver produced a list: every reachable file, in load order. That is
//! enough to parse and, when every top-level name is visible everywhere, enough to check.
//! It is not enough to ask the question visibility makes unavoidable — *may **this**
//! module name that definition?* — because the answer depends on an edge the list threw
//! away.
//!
//! So loading produces this. The edges are the `use` items, resolved to the modules they
//! name; everything after loading works on ids, and the filesystem is not consulted again.

use nazm_span::{FileId, Span};

use crate::key::ModuleKey;

/// One module of this compilation.
///
/// # Session-local, like everything else here
///
/// An index into one [`ModuleGraph`], valid for one compilation in one process. Two runs
/// over the same tree may number modules differently and both are correct.
///
/// # Why this is not [`FileId`]
///
/// Today they correspond one to one: a module comes into existence by being a file that
/// was loaded, and `ModuleId(n)` is the module of `FileId(n)`. They are still different
/// questions, and the separation costs one newtype:
///
/// - a `FileId` answers *where did this text come from* — it indexes a
///   [`SourceMap`](nazm_span::SourceMap), and a diagnostic uses it to name a file a reader
///   can open;
/// - a `ModuleId` answers *which unit owns this definition* — it indexes this graph, and
///   the checker uses it to decide what a name means.
///
/// The two come apart as soon as a module has no file behind it (generated source, an
/// editor's unsaved buffer) or a unit is more than one file (a package). Neither exists
/// yet, and neither is what this milestone builds. What the newtype buys today is that
/// nothing can silently use a file's position in load order as a semantic identity, which
/// is precisely the conflation `docs/research-register.md` R2 warns about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(u32);

impl ModuleId {
    /// The module the compilation started from: the file named on the command line.
    ///
    /// It owns `main`. See `docs/spec.md`, *The root module owns `main`*.
    pub const ROOT: Self = Self(0);

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One module named by another's `use`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dependency {
    /// The module named.
    pub on: ModuleId,
    /// The quoted path that named it, so a diagnostic about this import can point at the
    /// line that created it rather than at the module it brought in.
    pub at: Span,
}

/// One module: where its source came from, and what it directly imports.
#[derive(Debug, Clone)]
struct Module {
    file: FileId,
    /// Which logical module this is, across processes — when it has one.
    ///
    /// `None` for a module whose path escapes the source root, and for one reached under
    /// two different names through a symbolic link. The loader decides, because only it
    /// sees the second name. Neither case is an error: the compilation is complete and
    /// correct, and the module simply has no durable identity for something else to key
    /// on. See `docs/spec.md`, *Durable identity*.
    key: Option<ModuleKey>,
    /// Modules named by this one's `use` items, deduplicated, in source order.
    ///
    /// **Direct only.** What they in turn import is not here and is not this module's
    /// business — `docs/spec.md` states the rule and why it is the one with the largest
    /// effect on how much a reader of one module has to know.
    imports: Vec<Dependency>,
}

/// The modules of one compilation and the import edges between them.
///
/// Built by the driver while loading, because resolving `use "lib.nz"` means looking at
/// the filesystem and that is the driver's job. Read by everything after, because by then
/// a path is history.
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    modules: Vec<Module>,
}

impl ModuleGraph {
    /// A compilation of one module, with no imports.
    ///
    /// For the `&str` entry points — tests, mostly, and anything with genuinely one file.
    #[must_use]
    pub fn single() -> Self {
        Self {
            modules: vec![Module {
                file: FileId::ROOT,
                // A compilation assembled from a string has no source root to be relative
                // to, so the one module it contains has no durable identity. The `&str`
                // entry points are for tests and for programs with genuinely one file;
                // anything that came from disk goes through the loader, which does.
                key: None,
                imports: Vec::new(),
            }],
        }
    }

    /// Add a module for `file`, in load order. The first one added is the root.
    ///
    /// `key` is its durable identity, or `None` when it has none.
    pub fn add(&mut self, file: FileId, key: Option<ModuleKey>) -> ModuleId {
        let id = ModuleId(u32::try_from(self.modules.len()).unwrap_or(u32::MAX));
        self.modules.push(Module {
            file,
            key,
            imports: Vec::new(),
        });
        id
    }

    /// Withdraw a module's durable identity.
    ///
    /// For the loader, which discovers that a file has two names only when it reaches it
    /// the second time — by which point the module already exists. An identity the two
    /// compiler implementations could compute differently is worse than none.
    pub fn withdraw_key(&mut self, m: ModuleId) {
        if let Some(x) = self.modules.get_mut(m.index()) {
            x.key = None;
        }
    }

    /// Record that `from` names `to` in a `use`.
    ///
    /// Importing the same module twice adds one edge, and a module importing itself adds
    /// none: it already sees all of its own definitions, so the edge would only be a way
    /// for a module to collide with itself. `docs/spec.md` says both.
    pub fn import(&mut self, from: ModuleId, to: ModuleId, at: Span) {
        if from == to {
            return;
        }
        if let Some(m) = self.modules.get_mut(from.index()) {
            if !m.imports.iter().any(|d| d.on == to) {
                m.imports.push(Dependency { on: to, at });
            }
        }
    }

    /// Add the core prelude, read from `file`, and make every module already here import
    /// it.
    ///
    /// Called once, **after** every project module has been added, so it is the last module
    /// and adding it moves no other module's id or file. The edges are ordinary import
    /// edges: everything that follows — what a module may name, which interfaces its
    /// check depends on, what invalidates it — treats the prelude exactly as a module the
    /// project had written `use` for. Their span is synthetic because nothing was written.
    ///
    /// The prelude itself imports nothing. Calling this twice adds nothing the second
    /// time.
    pub fn attach_prelude(&mut self, file: FileId) -> ModuleId {
        if let Some(existing) = self.prelude() {
            return existing;
        }
        let core = self.add(file, Some(ModuleKey::prelude()));
        let users: Vec<ModuleId> = self.ids().filter(|m| *m != core).collect();
        for m in users {
            self.import(m, core, Span::synthetic());
        }
        core
    }

    /// The core prelude's module in this compilation, if it has one.
    ///
    /// Found by its durable key and by nothing else — not by position, and not by a file
    /// name — so a compilation assembled by hand, or from a persisted interface, has one
    /// exactly when it contains a module with that key.
    #[must_use]
    pub fn prelude(&self) -> Option<ModuleId> {
        self.modules
            .iter()
            .position(|m| m.key.as_ref().is_some_and(ModuleKey::is_prelude))
            .map(|i| ModuleId(u32::try_from(i).unwrap_or(u32::MAX)))
    }

    #[must_use]
    pub fn root(&self) -> ModuleId {
        ModuleId::ROOT
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Every module, in load order.
    pub fn ids(&self) -> impl Iterator<Item = ModuleId> + '_ {
        (0..self.modules.len()).map(|i| ModuleId(u32::try_from(i).unwrap_or(u32::MAX)))
    }

    /// The source this module was read from.
    #[must_use]
    pub fn file(&self, m: ModuleId) -> Option<FileId> {
        self.modules.get(m.index()).map(|x| x.file)
    }

    /// Which logical module this is, across processes.
    ///
    /// `None` is an answer, not a failure: see [`Module::key`]. A caller that needs one
    /// declines the work rather than substituting the session-local id, which would be a
    /// durable-looking identity that is not one.
    #[must_use]
    pub fn key(&self, m: ModuleId) -> Option<&ModuleKey> {
        self.modules.get(m.index()).and_then(|x| x.key.as_ref())
    }

    /// Which module owns the definitions written in this file.
    ///
    /// A linear scan, over a list that is one module per source file of one program. A
    /// map would be faster and would have to be kept correct; this cannot disagree with
    /// itself.
    #[must_use]
    pub fn module_of(&self, file: FileId) -> Option<ModuleId> {
        self.modules
            .iter()
            .position(|m| m.file == file)
            .map(|i| ModuleId(u32::try_from(i).unwrap_or(u32::MAX)))
    }

    /// The modules this one directly imports. Never transitive.
    #[must_use]
    pub fn imports(&self, m: ModuleId) -> &[Dependency] {
        self.modules
            .get(m.index())
            .map_or(&[][..], |x| x.imports.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere() -> Span {
        Span::synthetic()
    }

    fn on(g: &ModuleGraph, m: ModuleId) -> Vec<ModuleId> {
        g.imports(m).iter().map(|d| d.on).collect()
    }

    #[test]
    fn a_module_that_imports_itself_gains_no_edge() {
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        g.import(a, a, somewhere());
        assert!(g.imports(a).is_empty());
    }

    #[test]
    fn importing_the_same_module_twice_is_one_edge() {
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        let b = g.add(FileId::from_index(1), None);
        g.import(a, b, somewhere());
        g.import(a, b, somewhere());
        assert_eq!(on(&g, a), [b]);
    }

    #[test]
    fn a_cycle_is_representable() {
        // Not merely permitted by omission: the graph is built before anything is
        // checked, so a cycle is a shape it has to be able to hold.
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        let b = g.add(FileId::from_index(1), None);
        g.import(a, b, somewhere());
        g.import(b, a, somewhere());
        assert_eq!(on(&g, a), [b]);
        assert_eq!(on(&g, b), [a]);
    }

    #[test]
    fn imports_are_direct_and_the_graph_offers_no_transitive_view() {
        // A → B → C. The only question this type answers about A is which modules A
        // itself named, which is what makes non-transitivity a property of the data
        // rather than a rule the checker has to remember.
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        let b = g.add(FileId::from_index(1), None);
        let c = g.add(FileId::from_index(2), None);
        g.import(a, b, somewhere());
        g.import(b, c, somewhere());
        assert_eq!(on(&g, a), [b]);
        assert!(!on(&g, a).contains(&c));
    }

    #[test]
    fn the_prelude_is_imported_by_every_module_and_imports_nothing() {
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        let b = g.add(FileId::from_index(1), None);
        g.import(a, b, somewhere());
        let core = g.attach_prelude(FileId::from_index(2));
        assert_eq!(g.prelude(), Some(core));
        assert_eq!(on(&g, a), [b, core]);
        assert_eq!(on(&g, b), [core]);
        assert!(on(&g, core).is_empty());
        // Once only.
        assert_eq!(g.attach_prelude(FileId::from_index(3)), core);
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn a_module_knows_its_file_and_a_file_its_module() {
        let mut g = ModuleGraph::default();
        let a = g.add(FileId::ROOT, None);
        let b = g.add(FileId::from_index(7), None);
        assert_eq!(g.file(b), Some(FileId::from_index(7)));
        assert_eq!(g.module_of(FileId::from_index(7)), Some(b));
        assert_eq!(g.module_of(FileId::ROOT), Some(a));
        assert_eq!(g.module_of(FileId::from_index(3)), None);
    }
}
