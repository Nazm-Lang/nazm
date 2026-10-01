//! What the generated code calls a source file.
//!
//! # The name a compiled program carries
//!
//! Two strings reach a native artefact from the front end: the `source_filename` in its
//! header, and the location inside every runtime failure message. Until 2026-09-22 both
//! were `SourceMap::name` — *the path as the command line spelled it*. So
//!
//! ```text
//! nazm build app.nz              lib/util.nz:3:5
//! nazm build /tmp/a/app.nz       /tmp/a/lib/util.nz:3:5
//! nazm build ../a/app.nz         ../a/lib/util.nz:3:5
//! ```
//!
//! are three programs, byte for byte, that differ in no way a reader of the source could
//! see. The position was right in all three; what moved was the shell's opinion of where
//! the project is.
//!
//! # So the name is the module's durable identity
//!
//! [`ModuleKey`](nazm_sema::key::ModuleKey) is already the answer to *which module is
//! this*: the path relative to the declared source root, in one spelling on every
//! platform, with the checkout directory removed. Naming a module by it makes the three
//! lines above one line, and it is the same name `nazm check` files a cache entry under
//! and `NativeSymbol` spells into every symbol.
//!
//! The ordinary case does not change: a program built from its own directory already
//! printed `lib/util.nz`, because that is what the loader had been handed. What changes is
//! that it now prints that **whatever** it was handed.
//!
//! # Where there is no durable identity
//!
//! A module outside the source root, or reached under two names through a symbolic link,
//! has no [`ModuleKey`] — N3 decided to decline rather than guess, and this declines the
//! same way: such a file keeps the name the loader gave it. That is consistent with the
//! rest of the compiler, which already gives such a module a load-order native symbol
//! (`@nz.u.…`) rather than a durable one. A module with no durable identity produces no
//! reusable artefact, and this is one more place that is true rather than an exception to
//! it.

use nazm_sema::module::ModuleGraph;
use nazm_span::{FileId, LineCol, SourceMap, Span};

/// The name emission uses for each file, and the positions inside it.
///
/// A thin pairing rather than a new authority: positions still come from the
/// [`SourceMap`], which owns them, and the only thing decided here is *which of a file's
/// two names* a generated artefact carries.
#[derive(Debug)]
pub struct SourceNames<'a> {
    map: &'a SourceMap,
    /// Indexed by file, in load order: the durable name, where there is one.
    logical: Vec<Option<String>>,
}

impl<'a> SourceNames<'a> {
    /// Pair a source map with the graph that knows which module each file is.
    #[must_use]
    pub fn of(map: &'a SourceMap, graph: &ModuleGraph) -> Self {
        let mut logical = vec![None; map.len()];
        for m in graph.ids() {
            let (Some(file), Some(key)) = (graph.file(m), graph.key(m)) else {
                continue;
            };
            if let Some(slot) = logical.get_mut(file.index()) {
                *slot = Some(key.as_str().to_owned());
            }
        }
        Self { map, logical }
    }

    /// Where `span` is, as (file index, line, column), both counted from 1 — or `None` for a
    /// synthetic span, which has no place in any file (N48).
    #[must_use]
    pub fn position(&self, span: Span) -> Option<(u32, u32, u32)> {
        if span == Span::synthetic() {
            return None;
        }
        let at = self.map.line_col(span);
        Some((u32::try_from(span.file.index()).ok()?, at.line, at.column))
    }

    /// The name a debugger shows for the file at `index`: the loader's, which is the path
    /// the source can be found at.
    #[must_use]
    pub fn debug_file(&self, index: u32) -> String {
        self.map.name(FileId::from_index(index as usize)).to_owned()
    }

    /// A source map alone, for a caller that has no graph: every name is the loader's.
    ///
    /// Used by the tests that emit one unit directly. A compilation always has a graph.
    #[must_use]
    pub fn only(map: &'a SourceMap) -> Self {
        Self {
            map,
            logical: Vec::new(),
        }
    }

    /// What generated code calls this file.
    #[must_use]
    pub fn name(&self, file: FileId) -> &str {
        match self.logical.get(file.index()) {
            Some(Some(durable)) => durable,
            _ => self.map.name(file),
        }
    }

    /// Where in it, as the renderer would say it.
    #[must_use]
    pub fn line_col(&self, span: Span) -> LineCol {
        self.map.line_col(span)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use nazm_sema::key::ModuleKey;

    fn map() -> SourceMap {
        SourceMap::from_parts(vec![
            (
                "/tmp/a/app.nz".to_owned(),
                "fn main() -> Int { 0 }\n".to_owned(),
            ),
            (
                "/tmp/a/lib/util.nz".to_owned(),
                "fn helper() -> Int { 1 }\n".to_owned(),
            ),
        ])
    }

    #[test]
    fn a_module_with_a_durable_identity_is_named_by_it() {
        let map = map();
        let mut graph = ModuleGraph::default();
        graph.add(FileId::from_index(0), ModuleKey::relative("app.nz"));
        graph.add(FileId::from_index(1), ModuleKey::relative("lib/util.nz"));
        let names = SourceNames::of(&map, &graph);
        assert_eq!(names.name(FileId::from_index(0)), "app.nz");
        assert_eq!(names.name(FileId::from_index(1)), "lib/util.nz");
    }

    #[test]
    fn a_module_without_one_keeps_the_name_it_was_loaded_under() {
        // Declining rather than guessing, exactly as N3 decided: there is no relative
        // name to give, so the absolute one is better than a fabricated one.
        let map = map();
        let mut graph = ModuleGraph::default();
        graph.add(FileId::from_index(0), ModuleKey::relative("app.nz"));
        graph.add(FileId::from_index(1), None);
        let names = SourceNames::of(&map, &graph);
        assert_eq!(names.name(FileId::from_index(1)), "/tmp/a/lib/util.nz");
    }

    #[test]
    fn a_file_the_graph_does_not_know_keeps_its_loaded_name() {
        let map = map();
        let names = SourceNames::only(&map);
        assert_eq!(names.name(FileId::from_index(0)), "/tmp/a/app.nz");
        assert_eq!(names.name(FileId::from_index(1)), "/tmp/a/lib/util.nz");
    }
}
