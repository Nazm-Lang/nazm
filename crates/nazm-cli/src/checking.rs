//! `nazm check` — and the decision about which modules actually need checking.
//!
//! # Where the two halves meet
//!
//! `nazm-core` decides what a module *means*; `nazm-cache` decides whether that question
//! has already been answered. Neither knows about the other. This file is where they are
//! joined, and it is in the driver because the joining needs a filesystem: reading source
//! bytes, finding a store, writing an entry.
//!
//! # The algorithm, in the order it happens
//!
//! 1. every module's declarations are read — always, from source, with no reuse at all;
//! 2. that gives every module's **interface**, which depends on its own declarations and
//!    on nothing else. So every `InterfaceHash` is available at once, including in an
//!    import cycle, where no dependency order exists to compute them in;
//! 3. each module's [`CheckKey`] is its checker, its key, its source fingerprint and the
//!    `(ModuleKey, InterfaceHash)` of each direct import;
//! 4. an entry filed under that key means this module's bodies were checked, cleanly,
//!    against exactly these inputs — so they are not checked again;
//! 5. a module whose bodies *are* checked, and which produced no diagnostic in either
//!    stage, publishes an entry.
//!
//! # What is reused, stated precisely
//!
//! **Stage 2: checking a module's bodies.** Not reading the file, not parsing it, not
//! reading its declarations. Those are what produce the interface every *other* module's
//! key depends on, and recomputing them from source every run is what makes a skipped
//! module invisible — the compilation's own facts, including whether it has a `main`, are
//! never taken from a file on disk.
//!
//! That is a smaller claim than "incremental compilation" and it is the one that is true.
//! `docs/capability-matrix.md` says which.

use std::path::Path;
use std::process::ExitCode;

use nazm_cache::{
    CheckArtifact, CheckKey, CheckerIdentity, Dependency, Found, SourceFingerprint, Store,
};
use nazm_core::sema::interface::UnitInterface;
use nazm_core::sema::key::ModuleKey;
use nazm_core::sema::module::{ModuleGraph, ModuleId};
use nazm_core::sema::udt::UserTypes;
use nazm_iface::{InterfaceHash, PersistentUnitInterface};
use nazm_span::SourceMap;

use crate::root::How;
use crate::{exit, load, root};
use nazm_service::summary::Status;

/// What the run did, if anyone asked.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    modules: usize,
    /// Modules whose bodies were not checked, because the answer was already on disk.
    reused: usize,
    /// Checked because nothing was filed under their key.
    missed: usize,
    /// Checked because what was filed could not be used — damaged, or somebody else's.
    rejected: usize,
    /// Checked because the module has no durable identity to file anything under.
    declined: usize,
    /// Entries written this run.
    stored: usize,
}

/// Everything known about one module's participation in the cache.
#[derive(Debug, Default)]
struct Entry {
    key: Option<ModuleKey>,
    source: Option<SourceFingerprint>,
    /// What it offers, in the persisted form. `None` when it or one of its imports has no
    /// durable identity, which is also what makes it decline.
    published: Option<PersistentUnitInterface>,
    interface: Option<InterfaceHash>,
    dependencies: Option<Vec<Dependency>>,
    must_check: bool,
    /// A reused module's provenance facts (N38), as its entry kept them.
    provenance: Option<nazm_iface::PersistentFacts>,
    /// The module's resolved unit (N77): read from its entry when it was reused, written
    /// from the checker's resolution when it was checked cleanly.
    resolved: Option<nazm_iface::ResolvedUnit>,
}

/// The cache, as the checker sees it.
pub(crate) struct Cache<'a> {
    map: &'a SourceMap,
    store: Store,
    checker: CheckerIdentity,
    entries: Vec<Entry>,
    tally: Tally,
    /// The compilation's graph, for writing and reading facts by durable name.
    graph: ModuleGraph,
}

impl<'a> Cache<'a> {
    pub(crate) fn new(map: &'a SourceMap, store: Store) -> Self {
        Self {
            map,
            store,
            checker: CheckerIdentity::current(),
            entries: Vec::new(),
            tally: Tally::default(),
            graph: ModuleGraph::default(),
        }
    }

    fn at(&self, module: ModuleId) -> Option<&Entry> {
        self.entries.get(module.index())
    }

    /// Every module's resolved unit after a clean check, by module, and how many modules
    /// were checked and how many reused (N77). `None` for a module with no durable identity.
    pub(crate) fn units(&self) -> (Vec<Option<nazm_iface::ResolvedUnit>>, usize, usize) {
        (
            self.entries.iter().map(|e| e.resolved.clone()).collect(),
            self.tally.modules - self.tally.reused,
            self.tally.reused,
        )
    }
}

impl nazm_core::Reuse for Cache<'_> {
    fn plan(&mut self, graph: &ModuleGraph, interfaces: &[UnitInterface], types: &UserTypes) {
        self.graph = graph.clone();
        // First, what each module *is* and what it offers. Both are functions of that
        // module alone, so this pass needs no order and a cycle costs it nothing.
        self.entries = graph
            .ids()
            .map(|m| {
                let source = graph
                    .file(m)
                    .map(|f| SourceFingerprint::of(self.map.text(f).as_bytes()));
                let published = interfaces
                    .get(m.index())
                    .and_then(|i| PersistentUnitInterface::published(i, graph, types).ok());
                let interface = published.as_ref().and_then(|p| InterfaceHash::of(p).ok());
                Entry {
                    key: graph.key(m).cloned(),
                    source,
                    published,
                    interface,
                    // Every module is checked unless something below decides otherwise.
                    // The default is the safe one on purpose: a bug in what follows costs
                    // time, not correctness.
                    must_check: true,
                    ..Entry::default()
                }
            })
            .collect();
        self.tally.modules = self.entries.len();

        // Then each module's key, which needs its imports' interfaces — available now.
        for m in graph.ids() {
            let dependencies: Option<Vec<Dependency>> = graph
                .imports(m)
                .iter()
                .map(|d| {
                    let e = self.entries.get(d.on.index())?;
                    Some(Dependency {
                        module: e.key.clone()?,
                        interface: e.interface?,
                    })
                })
                .collect();

            let Some(e) = self.entries.get(m.index()) else {
                continue;
            };
            // A module with no durable key, or an import with none, or an interface that
            // could not be published: all three are the same answer, and it is N3's —
            // decline rather than guess. Such a module is checked from source every time,
            // and its compilation is otherwise unaffected.
            let (Some(key), Some(source), Some(deps)) = (e.key.clone(), e.source, dependencies)
            else {
                self.tally.declined += 1;
                continue;
            };

            let check_key = CheckKey::of(self.checker, &key, source, &deps);
            let found = self.store.get(&check_key);
            let mut provenance = None;
            let mut resolved = None;
            let reuse = match found {
                Found::Hit(r) => {
                    // The entry says what this module offers, and stage 1 has just
                    // computed the same thing from source. Requiring them to agree costs
                    // one comparison and turns the one failure a digest cannot rule out —
                    // two different inputs under one key — into a miss.
                    if e.published.as_ref() == Some(&r.interface) {
                        self.tally.reused += 1;
                        provenance = Some(r.provenance);
                        resolved = Some(r.resolved);
                        true
                    } else {
                        self.tally.rejected += 1;
                        false
                    }
                }
                Found::Miss => {
                    self.tally.missed += 1;
                    false
                }
                Found::Rejected(_) => {
                    self.tally.rejected += 1;
                    false
                }
            };

            if let Some(e) = self.entries.get_mut(m.index()) {
                e.dependencies = Some(deps);
                e.must_check = !reuse;
                e.provenance = provenance;
                e.resolved = resolved;
            }
        }
    }

    fn must_check(&self, module: ModuleId) -> bool {
        self.at(module).is_none_or(|e| e.must_check)
    }

    fn facts(
        &self,
        module: ModuleId,
        res: &nazm_core::sema::resolve::Resolution,
    ) -> Option<nazm_core::sema::ModuleFacts> {
        self.at(module)?
            .provenance
            .as_ref()?
            .rehydrate(module, res, &self.graph)
    }

    fn checked_clean(
        &mut self,
        module: ModuleId,
        facts: &nazm_core::sema::ModuleFacts,
        res: &nazm_core::sema::resolve::Resolution,
    ) {
        // A closure exists only while its maker's body is checked: a module reused from the
        // store has none, so facts naming one cannot be read back, and the whole program's
        // flow would be solved as incomplete — a program accepted cold refused warm (Q1-B-C1).
        // Such a module files no entry and is checked every time: declined, in the report's words.
        let closure =
            |g: nazm_core::sema::FnId| res.function(g).is_some_and(|d| d.closure.is_some());
        if facts
            .functions
            .iter()
            .any(|f| closure(f.function) || f.makes.iter().any(|m| closure(m.function)))
        {
            self.tally.declined += 1;
            return;
        }
        let Some(persisted) = nazm_iface::PersistentFacts::of(facts, res, &self.graph) else {
            return;
        };
        let Some(e) = self.entries.get(module.index()) else {
            return;
        };
        let (Some(key), Some(source), Some(deps), Some(published)) = (
            e.key.as_ref(),
            e.source,
            e.dependencies.as_ref(),
            e.published.as_ref(),
        ) else {
            return;
        };
        let Ok(artifact) = CheckArtifact::of(self.checker, key, source, deps, published.clone())
        else {
            return;
        };
        // The module's names, from the checker's own answers (N77). A module that cannot
        // name its targets durably keeps no unit and so no entry: an entry without one
        // would be a miss on every read.
        let index = nazm_core::sema::ReferenceIndex::of(res);
        let map = self.map;
        let entries = &self.entries;
        let interface = |m: ModuleId| {
            entries
                .get(m.index())
                .and_then(|e| e.interface)
                .map(|h| h.to_string())
        };
        let written = |at: nazm_span::Span| {
            map.text(at.file)
                .get(at.start as usize..at.end as usize)
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned()
        };
        let Ok(unit) = nazm_iface::ResolvedUnit::of(
            res,
            &index,
            &self.graph,
            module,
            &source.to_string(),
            &interface,
            &written,
        ) else {
            return;
        };
        let artifact = artifact
            .with_provenance(persisted)
            .with_resolved(unit.clone());
        if let Some(e) = self.entries.get_mut(module.index()) {
            e.resolved = Some(unit);
        }
        // A store that will not take an entry is not a reason to fail a compilation: the
        // disk is full, or the tree is read-only, and the program is fine either way.
        if self.store.put(&artifact).is_ok() {
            self.tally.stored += 1;
        }
    }
}

/// One `nazm.check-report/1` object.
///
/// Structured because the reader is as likely to be a script or an agent as a person, and
/// versioned because anything a script reads is an interface. Off unless asked for: a
/// compiler that reports on itself on every run is a compiler whose output has to be
/// filtered before it can be read.
///
/// `reused` is **the cache's decision** — the modules it established need no checking.
/// The checker overrides it in one case, a module whose source did not parse, and such a
/// compilation has already failed.
fn report(root: How, t: Tally) -> String {
    format!(
        r#"{{"schema":"nazm.check-report/1","root":"{}","modules":{},"checked":{},"reused":{},"missed":{},"rejected":{},"declined":{},"stored":{}}}"#,
        root.as_str(),
        t.modules,
        t.modules - t.reused,
        t.reused,
        t.missed,
        t.rejected,
        t.declined,
        t.stored,
    )
}

pub(crate) fn run(
    path: &Path,
    json: bool,
    declared: Option<&Path>,
    cache: bool,
    with_report: bool,
    summary: bool,
) -> ExitCode {
    // `--summary-json` (N31): the same outcome at every exit below, reported as one
    // `nazm.command-summary/1` in place of the rendering. The exit status is the same.
    let summarized = |status, code, diagnostics: &[nazm_diag::Diagnostic], map| {
        crate::summary::finish(
            nazm_service::summary::Command::Check,
            status,
            code,
            diagnostics,
            map,
            None,
            path,
            declared,
        )
    };
    let source_root = match root::decide(path, declared) {
        Ok(r) => r,
        Err(e) => {
            let code = exit::root_status(&e);
            if summary {
                let status = if code == exit::NO_SUCH_FILE {
                    Status::Unreadable
                } else {
                    Status::Refused
                };
                return summarized(status, code, &[], None);
            }
            eprintln!("nazm: {e}");
            return ExitCode::from(code);
        }
    };

    let loaded = match load::load(path, source_root.path()) {
        Ok(f) => f,
        Err((partial, diagnostics)) => {
            let code = exit::load_status(&diagnostics);
            if summary {
                let status = if code == exit::NO_SUCH_FILE {
                    Status::Unreadable
                } else {
                    Status::Rejected
                };
                return summarized(status, code, &diagnostics, Some(&partial.map));
            }
            crate::emit(&diagnostics, &partial.map, json);
            return ExitCode::from(code);
        }
    };
    let map = &loaded.map;

    let (diagnostics, tally) = if cache {
        let mut c = Cache::new(map, crate::derived::checks(source_root.path()));
        let d = nazm_core::check_map_reusing(map, &loaded.graph, &mut c);
        (d, c.tally)
    } else {
        // The route every other caller takes, unchanged since before there was a cache.
        // It is what the equivalence tests compare against, which is the only reason a
        // compiler needs a way to turn its cache off.
        let d = nazm_core::check_map(map, &loaded.graph);
        (
            d,
            Tally {
                modules: loaded.graph.len(),
                ..Tally::default()
            },
        )
    };

    if summary {
        let (status, code) = if diagnostics.is_empty() {
            (Status::Success, exit::OK)
        } else {
            (Status::Rejected, exit::CHECK_FAILED)
        };
        return summarized(status, code, &diagnostics, Some(map));
    }

    if with_report {
        println!("{}", report(source_root.how(), tally));
    }

    if diagnostics.is_empty() {
        println!("ok");
        return ExitCode::from(exit::OK);
    }
    let n = diagnostics.len();
    crate::emit(&diagnostics, map, json);
    eprintln!("nazm: {n} error{}", if n == 1 { "" } else { "s" });
    ExitCode::from(exit::CHECK_FAILED)
}

/// `nazm check DIR` for a library package (N74, `docs/architecture.md` §7.76): each module of its
/// source root checked as a module — no `main` required — with everything it imports, and each
/// diagnostic reported once however many modules reach it.
pub(crate) fn library(modules: &[std::path::PathBuf], json: bool) -> ExitCode {
    let mut seen = std::collections::BTreeSet::new();
    let mut errors = 0usize;
    for path in modules {
        let source_root = match crate::root::decide(path, None) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("nazm: {e}");
                return ExitCode::from(crate::exit::root_status(&e));
            }
        };
        let loaded = match crate::load::load(path, source_root.path()) {
            Ok(l) => l,
            Err((partial, diagnostics)) => {
                crate::emit(&diagnostics, &partial.map, json);
                return ExitCode::from(crate::exit::load_status(&diagnostics));
            }
        };
        let map = &loaded.map;
        let Err(diagnostics) = nazm_core::on_module_map(map, &loaded.graph, |_, _| Ok(())) else {
            continue;
        };
        let fresh: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| {
                let s = d.primary.span;
                seen.insert((
                    map.name(s.file).to_owned(),
                    s.start,
                    s.end,
                    d.code.to_string(),
                ))
            })
            .collect();
        errors += fresh.len();
        crate::emit(&fresh, map, json);
    }
    if errors == 0 {
        println!("ok");
        return ExitCode::from(exit::OK);
    }
    eprintln!("nazm: {errors} error{}", if errors == 1 { "" } else { "s" });
    ExitCode::from(exit::CHECK_FAILED)
}
