//! Call hierarchy: a source function, and the direct calls the checker recorded into and out
//! of it. N25.
//!
//! # The contract
//!
//! **An edge exists only because the checker recorded a call from one source function to
//! another.** Two facts the checker already keeps, and nothing else, make one:
//!
//! - a [`CheckedCall`](nazm_core::sema::CheckedCall) (N20), keyed by the callee's name span,
//!   says a call was checked there and which function it resolved to;
//! - the body the checker was checking when it recorded that call keeps the same span among
//!   its [`call_sites`](nazm_core::sema::FnDef::call_sites) — recorded at the same point,
//!   under the same resolved callee, with the function being checked as the owner — so the
//!   caller is read, not worked out from where the span happens to sit.
//!
//! A call site with no checked call is no edge; a checked call to a built-in is no edge
//! either, since a built-in has no declaration to navigate to. No name is compared, no
//! reference is taken for a call, and no syntax is scanned for call-shaped text: a record or
//! variant construction is not a call to the checker and is not one here.
//!
//! # Items
//!
//! A hierarchy item is a function the checker declared from source this compilation parsed,
//! with the declaration and name ranges its outline symbol has (N22 — the same function
//! reads both). A built-in, a core-prelude definition, and a function described only by a
//! persisted interface have no file an editor can open, are never items, and are left out
//! of every edge list rather than given a place that does not exist.
//!
//! Preparing reads the entity the reference index records at the name under the cursor
//! (N17): a function's declaration or any call of it. Anything else — a record, a variant,
//! a field, a local — prepares nothing.
//!
//! # Which program
//!
//! **One compilation: the one rooted at the document the item was prepared from**, at the
//! generation it was prepared at. Its callers are the calls known in that compilation
//! snapshot — the root and everything it imports — and nothing else: not another open
//! document's compilation that loads the same file, not a directory walk. So one shared
//! function prepared through two roots is two items with different callers, and neither
//! list is a claim about every caller in a project. N18's refusal to rename an exported
//! definition is not affected.
//!
//! # Items across requests
//!
//! A prepared item carries a [`Locator`]: the root document, its version, the service
//! generation, and the declaring file and name bytes — a position, never a session-local
//! id or a spelling. Incoming and outgoing calls are answered only while all of that is
//! still current, from the root's own current analysis; after any change, or once the root
//! is closed, the item has no answer and must be prepared again. Nothing is kept between
//! requests: no item table, and no graph — each answer is derived and dropped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nazm_core::sema::{Callee, FnId, ReferenceTarget};
use nazm_span::{FileId, Span};

use crate::service::{identity, name_at, FileKey, LanguageService, Location, Snapshot, Target};

/// Where a prepared item came from, as a position a later request can check: which
/// compilation, at which generation, and which declaration in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Locator {
    /// The identity path of the document whose compilation the item belongs to.
    pub root: PathBuf,
    /// That document's version when the item was prepared.
    pub version: i32,
    /// The service generation the item was prepared at.
    pub generation: u64,
    /// The identity path of the file the function is declared in.
    pub file: PathBuf,
    /// The declared name's bytes in that file.
    pub name_start: u32,
    pub name_end: u32,
}

/// A source function, as a call hierarchy item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyFunction {
    pub locator: Locator,
    pub name: String,
    /// Its signature, as the shared renderer writes it.
    pub detail: Option<String>,
    /// The whole declaration.
    pub declaration: Location,
    /// The declared name.
    pub selection: Location,
}

/// A function that calls the item, and where: each callee name, in the caller's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingCall {
    pub from: HierarchyFunction,
    /// In source order, each once.
    pub calls: Vec<Location>,
}

/// A function the item calls, and where: each callee name, in the item's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingCall {
    pub to: HierarchyFunction,
    /// In source order, each once.
    pub calls: Vec<Location>,
}

/// Why there is no call hierarchy answer. There are never historical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHierarchy {
    /// The document is not open at that version, or the item was prepared from an
    /// analysis that is no longer current.
    NotCurrent,
}

impl LanguageService {
    /// The source function the name at byte `offset` of `path`, at `version`, declares or
    /// calls — `None` for any other name, or a function with no source.
    ///
    /// # Errors
    ///
    /// [`NoHierarchy::NotCurrent`] unless the document is open at `version` and analysed at
    /// the current generation.
    pub fn prepare_call_hierarchy(
        &self,
        path: &Path,
        version: i32,
        offset: u32,
    ) -> Result<Option<HierarchyFunction>, NoHierarchy> {
        let s = self.current(path, version).ok_or(NoHierarchy::NotCurrent)?;
        let root = identity(path);
        Ok(name_at(&s.cst, offset)
            .and_then(|at| s.index.target(at))
            .and_then(|entity| match entity {
                ReferenceTarget::Function(id) => hierarchy_item(s, &root, version, id),
                _ => None,
            }))
    }

    /// Every source function of the item's compilation with a checked call to it, each
    /// with those calls — ordered by the caller's file, then where its name is, then name.
    ///
    /// # Errors
    ///
    /// [`NoHierarchy::NotCurrent`] unless the item's root is open at the version, and the
    /// service at the generation, it was prepared at.
    pub fn incoming_calls(&self, item: &Locator) -> Result<Vec<IncomingCall>, NoHierarchy> {
        let (s, target) = self.bound(item)?;
        let res = &s.analysis.resolution;
        let mut callers: BTreeMap<FnId, Vec<Span>> = BTreeMap::new();
        for (i, def) in res.functions().iter().enumerate() {
            for site in &def.call_sites {
                if res.checked_call(site.span).map(|c| c.callee) == Some(Callee::Function(target)) {
                    callers
                        .entry(FnId::from_index(i))
                        .or_default()
                        .push(site.span);
                }
            }
        }
        let mut out: Vec<IncomingCall> = callers
            .into_iter()
            .filter_map(|(caller, spans)| {
                Some(IncomingCall {
                    from: hierarchy_item(s, &item.root, item.version, caller)?,
                    calls: locations(s, spans),
                })
            })
            .collect();
        out.sort_by(|a, b| order(&a.from, &b.from));
        Ok(out)
    }

    /// Every source function the item has a checked call to, each with those calls —
    /// ordered by the callee's file, then where its name is, then name.
    ///
    /// # Errors
    ///
    /// As [`Self::incoming_calls`].
    pub fn outgoing_calls(&self, item: &Locator) -> Result<Vec<OutgoingCall>, NoHierarchy> {
        let (s, caller) = self.bound(item)?;
        let res = &s.analysis.resolution;
        let mut callees: BTreeMap<FnId, Vec<Span>> = BTreeMap::new();
        for site in res.function(caller).map_or(&[][..], |f| &f.call_sites) {
            if let Some(Callee::Function(to)) = res.checked_call(site.span).map(|c| c.callee) {
                callees.entry(to).or_default().push(site.span);
            }
        }
        let mut out: Vec<OutgoingCall> = callees
            .into_iter()
            .filter_map(|(callee, spans)| {
                Some(OutgoingCall {
                    to: hierarchy_item(s, &item.root, item.version, callee)?,
                    calls: locations(s, spans),
                })
            })
            .collect();
        out.sort_by(|a, b| order(&a.to, &b.to));
        Ok(out)
    }

    /// The current analysis the item was prepared from, and the function it names in it —
    /// found at its declaration's bytes in its file, never by its name.
    fn bound(&self, item: &Locator) -> Result<(&Snapshot, FnId), NoHierarchy> {
        if item.generation != self.generation {
            return Err(NoHierarchy::NotCurrent);
        }
        let doc = self
            .documents
            .get(&item.root)
            .ok_or(NoHierarchy::NotCurrent)?;
        let s = self
            .current(&doc.path, item.version)
            .ok_or(NoHierarchy::NotCurrent)?;
        let file = FileKey::Path(item.file.clone());
        let f = s
            .files
            .iter()
            .position(|k| *k == file)
            .ok_or(NoHierarchy::NotCurrent)?;
        let at = Span::new(FileId::from_index(f), item.name_start, item.name_end);
        match s.index.target(at) {
            Some(entity @ ReferenceTarget::Function(id))
                if s.index.declaration(entity) == Some(at) =>
            {
                Ok((s, id))
            }
            _ => Err(NoHierarchy::NotCurrent),
        }
    }
}

/// The function `id` of `s` as an item bound to the compilation rooted at `root` —
/// `None` unless it was declared from a file an editor can open.
fn hierarchy_item(s: &Snapshot, root: &Path, version: i32, id: FnId) -> Option<HierarchyFunction> {
    let symbol = crate::symbols::function_symbol(s, id)?;
    let file = s.analysis.resolution.function(id)?.name_span.file;
    let FileKey::Path(path) = s.files.get(file.index())? else {
        return None;
    };
    let (Target::File(declaration), Target::File(selection)) = (
        s.target(Span::new(file, symbol.start, symbol.end))?,
        s.target(Span::new(file, symbol.name_start, symbol.name_end))?,
    ) else {
        return None;
    };
    Some(HierarchyFunction {
        locator: Locator {
            root: root.to_path_buf(),
            version,
            generation: s.generation,
            file: path.clone(),
            name_start: symbol.name_start,
            name_end: symbol.name_end,
        },
        name: symbol.name,
        detail: symbol.detail,
        declaration,
        selection,
    })
}

/// Call spans as places in their file, in source order, each once.
fn locations(s: &Snapshot, mut spans: Vec<Span>) -> Vec<Location> {
    spans.sort_by_key(|at| (at.file, at.start, at.end));
    spans.dedup();
    spans
        .into_iter()
        .filter_map(|at| match s.target(at)? {
            Target::File(l) => Some(l),
            Target::Prelude { .. } => None,
        })
        .collect()
}

/// Items by declaring file, then where the name is, then name.
fn order(a: &HierarchyFunction, b: &HierarchyFunction) -> std::cmp::Ordering {
    (&a.locator.file, a.locator.name_start, &a.name).cmp(&(
        &b.locator.file,
        b.locator.name_start,
        &b.name,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    /// The structural half of "only checked calls": this module never compares a name,
    /// looks a function up by one, takes the uses of an entity for calls, or reads syntax.
    #[test]
    fn an_edge_is_never_found_by_name_reference_or_syntax() {
        let source = include_str!("hierarchy.rs");
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
            ".name ==",
            "== name",
            ".definition(",
            "by_module",
            ".uses(",
            ".callee(",
            "program.functions",
            "Expr::",
            "parse_",
            "read_dir",
            "Disk",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in hierarchy: an edge or an item would not be the checker's"
            );
        }
    }
}
