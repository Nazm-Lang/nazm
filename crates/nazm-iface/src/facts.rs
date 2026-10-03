//! A module's provenance facts (N38), in a form that outlives the process.
//!
//! [`nazm_sema::ModuleFacts`] name functions by session-local [`FnId`] and places by
//! [`Span`], neither of which survives a run. Here a function is its durable name — its
//! module's key and its own — and a place is a byte range in the module's own file, which the
//! module's key already fixes. The facts are a function of the module's source and its imports'
//! declarations and nothing else, so they sit in a cache entry beside the interface that entry
//! was checked against, and are read back only for a module whose source is that one.
//!
//! Read back strictly: an unknown origin, a call index out of range, or a callee this
//! compilation does not have, is a refusal — never a guess — and the module is checked again.

use serde::{Deserialize, Serialize};

use nazm_sema::key::ModuleKey;
use nazm_sema::module::{ModuleGraph, ModuleId};
use nazm_sema::resolve::{FnId, Resolution};
use nazm_sema::{
    CallFact, Flow, FunctionFacts, IndirectFact, MakeFact, ModuleFacts, Origin, OriginSet,
    SinkClass, SinkFact, StoreFact,
};
use nazm_span::{FileId, Span};

/// A module's facts, persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PersistentFacts {
    pub functions: Vec<PersistentFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentFunction {
    /// Its name in its module.
    pub name: String,
    pub calls: Vec<PersistentCall>,
    pub result: PersistentFlow,
    pub sinks: Vec<PersistentSink>,
    /// What it writes into containers' cells (N80).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stores: Vec<PersistentStore>,
    /// Its calls through values and bounds (N80).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indirect: Vec<PersistentIndirect>,
    /// The function values it makes (N80).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub makes: Vec<PersistentMake>,
}

/// A value written into a cell (N80). A cell is named by its container's type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentStore {
    pub cell: String,
    pub at: [u32; 2],
    pub value: PersistentFlow,
}

/// A call through a value or a bound (N80), by the target it answers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentIndirect {
    pub target: String,
    pub at: [u32; 2],
    pub args: Vec<PersistentFlow>,
}

/// A function value made (N80): the function's module key and name — a closure's is the
/// name its maker gave it — its target, and what it captures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentMake {
    pub module: String,
    pub function: String,
    pub target: String,
    pub at: [u32; 2],
    pub captures: Vec<PersistentFlow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentCall {
    /// The callee's module key, and its name there.
    pub module: String,
    pub callee: String,
    /// `[start, end)` in this module's file.
    pub at: [u32; 2],
    pub args: Vec<PersistentFlow>,
    pub spawned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentSink {
    pub at: [u32; 2],
    pub path: PersistentFlow,
    /// Which sink (N52): `path`, `output`, `file-data` or `vouch`. **Absent** for a path sink,
    /// so a facts entry with only path sinks is written as it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PersistentFlow {
    /// Each origin met in the body, by name in id order, with where it was first met.
    pub origins: Vec<(String, [u32; 2])>,
    pub params: Vec<u32>,
    pub calls: Vec<u32>,
    /// The cells it was read out of, by name, sorted, with where each was first read (N80).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<(String, [u32; 2])>,
    /// The indirect calls whose results it carries (N80).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<u32>,
}

impl PersistentFlow {
    fn of(f: &Flow) -> Self {
        Self {
            origins: f
                .why
                .iter()
                .map(|(o, at)| (o.name().to_owned(), [at.start, at.end]))
                .collect(),
            params: f.params.clone(),
            calls: f.calls.clone(),
            cells: f
                .cells
                .iter()
                .map(|(c, at)| (c.clone(), [at.start, at.end]))
                .collect(),
            via: f.via.clone(),
        }
    }

    fn back(&self, file: FileId, calls: usize, via: usize) -> Option<Flow> {
        let mut origins = OriginSet::EMPTY;
        let mut why = Vec::new();
        for (name, [s, e]) in &self.origins {
            let o = Origin::by_name(name)?;
            if origins.contains(o) || s > e {
                return None;
            }
            origins = origins.with(o);
            why.push((o, Span::new(file, *s, *e)));
        }
        if why
            .windows(2)
            .any(|w| w.first().map(|a| a.0.id()) >= w.get(1).map(|b| b.0.id()))
            || self.params.windows(2).any(|w| w.first() >= w.get(1))
            || self.calls.windows(2).any(|w| w.first() >= w.get(1))
            || self.calls.iter().any(|c| *c as usize >= calls)
            || self.via.windows(2).any(|w| w.first() >= w.get(1))
            || self.via.iter().any(|c| *c as usize >= via)
            || self
                .cells
                .windows(2)
                .any(|w| w.first().map(|c| &c.0) >= w.get(1).map(|c| &c.0))
            || self.cells.iter().any(|(_, [s, e])| s > e)
        {
            return None;
        }
        Some(Flow {
            origins,
            why,
            params: self.params.clone(),
            calls: self.calls.clone(),
            cells: self
                .cells
                .iter()
                .map(|(c, [s, e])| (c.clone(), Span::new(file, *s, *e)))
                .collect(),
            via: self.via.clone(),
        })
    }
}

fn module_of(graph: &ModuleGraph, key: &str) -> Option<ModuleId> {
    graph
        .ids()
        .find(|m| graph.key(*m).is_some_and(|k| k.to_string() == key))
}

/// A function of `module` by its durable name: a definition, or a closure (N50), which is in
/// no namespace and is found by the name its maker gave it.
fn function_named(res: &Resolution, module: ModuleId, name: &str) -> Option<FnId> {
    res.definition(module, name).or_else(|| {
        res.functions()
            .iter()
            .position(|d| d.module == module && d.closure.is_some() && d.name == name)
            .map(FnId::from_index)
    })
}

fn span_of(file: FileId, [s, e]: [u32; 2]) -> Option<Span> {
    (s <= e).then(|| Span::new(file, s, e))
}

impl PersistentFacts {
    /// `facts`, written durably. `None` when a function or a callee has no durable name,
    /// which is the same answer the interface gives: nothing is filed for such a module.
    #[must_use]
    pub fn of(facts: &ModuleFacts, res: &Resolution, graph: &ModuleGraph) -> Option<Self> {
        let key_of = |g: FnId| -> Option<(String, String)> {
            let d = res.function(g)?;
            let k: &ModuleKey = graph.key(d.module)?;
            Some((k.to_string(), d.name.clone()))
        };
        let functions = facts
            .functions
            .iter()
            .map(|f| {
                let (_, name) = key_of(f.function)?;
                let calls = f
                    .calls
                    .iter()
                    .map(|c| {
                        let (module, callee) = key_of(c.callee)?;
                        Some(PersistentCall {
                            module,
                            callee,
                            at: [c.at.start, c.at.end],
                            args: c.args.iter().map(PersistentFlow::of).collect(),
                            spawned: c.spawned,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                let makes = f
                    .makes
                    .iter()
                    .map(|m| {
                        let (module, function) = key_of(m.function)?;
                        Some(PersistentMake {
                            module,
                            function,
                            target: m.target.clone(),
                            at: [m.at.start, m.at.end],
                            captures: m.captures.iter().map(PersistentFlow::of).collect(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(PersistentFunction {
                    name,
                    calls,
                    stores: f
                        .stores
                        .iter()
                        .map(|st| PersistentStore {
                            cell: st.cell.clone(),
                            at: [st.at.start, st.at.end],
                            value: PersistentFlow::of(&st.value),
                        })
                        .collect(),
                    indirect: f
                        .indirect
                        .iter()
                        .map(|c| PersistentIndirect {
                            target: c.target.clone(),
                            at: [c.at.start, c.at.end],
                            args: c.args.iter().map(PersistentFlow::of).collect(),
                        })
                        .collect(),
                    makes,
                    result: PersistentFlow::of(&f.result),
                    sinks: f
                        .sinks
                        .iter()
                        .map(|s| PersistentSink {
                            at: [s.at.start, s.at.end],
                            path: PersistentFlow::of(&s.path),
                            class: (s.class != SinkClass::Path).then(|| s.class.name().to_owned()),
                        })
                        .collect(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { functions })
    }

    /// Back into this compilation's terms, for `module`. `None` for anything that does not
    /// fit it — which a caller treats as no entry at all.
    #[must_use]
    pub fn rehydrate(
        &self,
        module: ModuleId,
        res: &Resolution,
        graph: &ModuleGraph,
    ) -> Option<ModuleFacts> {
        let file = graph.file(module)?;
        let mut functions = Vec::with_capacity(self.functions.len());
        for f in &self.functions {
            let function = function_named(res, module, &f.name)?;
            let n = f.calls.len();
            let v = f.indirect.len();
            let calls = f
                .calls
                .iter()
                .map(|c| {
                    let m = module_of(graph, &c.module)?;
                    let callee = res.definition(m, &c.callee)?;
                    let [s, e] = c.at;
                    if s > e {
                        return None;
                    }
                    Some(CallFact {
                        callee,
                        at: Span::new(file, s, e),
                        args: c
                            .args
                            .iter()
                            .map(|a| a.back(file, n, v))
                            .collect::<Option<Vec<_>>>()?,
                        spawned: c.spawned,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let sinks = f
                .sinks
                .iter()
                .map(|s| {
                    let [a, b] = s.at;
                    if a > b {
                        return None;
                    }
                    Some(SinkFact {
                        at: Span::new(file, a, b),
                        path: s.path.back(file, n, v)?,
                        // An unknown class is no entry at all, never a path sink by default.
                        class: match &s.class {
                            None => SinkClass::Path,
                            Some(c) => SinkClass::by_name(c)?,
                        },
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let stores = f
                .stores
                .iter()
                .map(|st| {
                    Some(StoreFact {
                        cell: st.cell.clone(),
                        at: span_of(file, st.at)?,
                        value: st.value.back(file, n, v)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let indirect = f
                .indirect
                .iter()
                .map(|c| {
                    Some(IndirectFact {
                        target: c.target.clone(),
                        at: span_of(file, c.at)?,
                        args: c
                            .args
                            .iter()
                            .map(|a| a.back(file, n, v))
                            .collect::<Option<Vec<_>>>()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let makes = f
                .makes
                .iter()
                .map(|m| {
                    let owner = module_of(graph, &m.module)?;
                    Some(MakeFact {
                        function: function_named(res, owner, &m.function)?,
                        target: m.target.clone(),
                        at: span_of(file, m.at)?,
                        captures: m
                            .captures
                            .iter()
                            .map(|a| a.back(file, n, v))
                            .collect::<Option<Vec<_>>>()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            functions.push(FunctionFacts {
                function,
                calls,
                result: f.result.back(file, n, v)?,
                sinks,
                stores,
                indirect,
                makes,
            });
        }
        Some(ModuleFacts { functions })
    }
}
