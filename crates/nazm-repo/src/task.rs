//! `nazm.task-context/1`: for structured seeds and an intent, the smallest context the
//! repository can justify, each item with why it is there.
//!
//! # Closure
//!
//! One step from each seed, never more: what the seed directly uses, what directly uses it
//! when the intent is an edit, the tests the catalogue relates to it, and the sections its
//! owner declares or its text links. Nothing is followed from an item that is not a seed —
//! a dependency's dependencies are its own task — so a cycle is one visit, and a
//! relationship with more than [`FAN_OUT`] members is cut there and counted.
//!
//! | Intent | From a definition |
//! | --- | --- |
//! | `understand` | the seed's exact source and shape; the records and enums its signature and body name, and every other definition it uses, by shape; its source root's documentation |
//! | `edit` (the default) | `understand`, and what uses it — callers of a function, the definitions a type is used in, in every compilation of the manifest that loads its module — and its tests |
//! | `diagnose` | the seed, the diagnostics inside it, and what its signature names |
//! | `test` | the seed, and the tests the catalogue relates to it: killers of the mutations inside it, and the targets of the profile that owns its file |
//!
//! Omitting the intent is `edit`, the widest: an omitted intent never drops what an edit
//! needs to be safe.
//!
//! # Priority and budget
//!
//! Every item has a class, and the classes have one fixed order: `seed`, `correctness`,
//! `authority` (normative, grammar, law, interface, architecture and evidence sections, in
//! that order), `edit_safety`, `test`, `supporting` (guide and operational sections) and
//! `context` (planning, research and goal sections — never evidence of what exists). Within
//! a class, by that authority order and then by id. A budget counts entities, the bytes of
//! included definition source and the bytes of included section text. The seeds are
//! mandatory: a budget they do not fit is refused with the minimum they need. After them,
//! items are taken in order until one does not fit; it and every later item are omitted and
//! listed, and the result is `partial` — never a truncated context that says it is
//! complete.

use std::collections::BTreeSet;
use std::path::Path;

use nazm_service::context::{ContextPacket, ContextResult};
use nazm_service::{DetailResult, DurableDefinition, IndexResult};
use serde::{Deserialize, Serialize};

use crate::manifest::SourceRoot;
use crate::map::{authority_word, retrieval, Retrieval};
use crate::repository::Repository;
use crate::sources::Compilation;
use crate::{Layout, Problem, TASK_SCHEMA};

/// At most this many seeds in one request.
pub const MAX_SEEDS: usize = 8;
/// At most this many members of one relationship of one seed; the rest are counted.
pub const FAN_OUT: usize = 16;

/// What the context is for. It decides which relationships are followed, never their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Understand,
    Edit,
    Diagnose,
    Test,
}

impl Intent {
    /// The widest intent.
    pub const DEFAULT: Self = Self::Edit;

    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "understand" => Some(Self::Understand),
            "edit" => Some(Self::Edit),
            "diagnose" => Some(Self::Diagnose),
            "test" => Some(Self::Test),
            _ => None,
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Understand => "understand",
            Self::Edit => "edit",
            Self::Diagnose => "diagnose",
            Self::Test => "test",
        }
    }
}

/// A structural budget: entities, and bytes of definition source and of section text. Not
/// tokens. `None` is unbounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    #[serde(default)]
    pub max_entities: Option<usize>,
    #[serde(default)]
    pub max_source_bytes: Option<usize>,
    #[serde(default)]
    pub max_doc_bytes: Option<usize>,
}

/// One request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRequest {
    pub intent: Option<Intent>,
    /// Repository entity ids, from a map or an earlier context.
    pub seeds: Vec<String>,
    /// The state the ids are of; another state is `stale_state`.
    pub state: Option<String>,
    pub budget: Budget,
}

/// A context, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum TaskResult {
    Available(Box<TaskContext>),
    Refused(Refusal),
}

/// The context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskContext {
    pub schema: &'static str,
    /// `complete`, or `partial` when anything the closure selected is not here.
    pub status: &'static str,
    pub state: String,
    pub intent: &'static str,
    pub seeds: Vec<String>,
    pub budget: Budget,
    pub used: Used,
    /// The seeds in request order, then the rest in priority order.
    pub items: Vec<Item>,
    /// Selected and left out by the budget, in priority order.
    pub omitted: Vec<TaskOmission>,
    /// Relationships cut at [`FAN_OUT`].
    pub bounded: Vec<Bound>,
    /// Known and deliberately not selected, counted.
    pub excluded: Vec<Exclusion>,
}

/// What the included items cost, in the budget's units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Used {
    pub entities: usize,
    pub source_bytes: usize,
    pub doc_bytes: usize,
}

/// One item of context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: &'static str,
    /// Why it is here.
    pub reason: &'static str,
    /// The seed it was reached from; `None` for a seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    pub class: &'static str,
    pub authority: &'static str,
    /// The compilation a semantic fact was read from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<Retrieval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "is_zero")]
    pub source_bytes: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub doc_bytes: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskOmission {
    pub id: String,
    pub reason: &'static str,
    pub via: Option<String>,
    pub class: &'static str,
    /// `entities`, `source_bytes` or `doc_bytes` — what it did not fit — or `priority`:
    /// it would fit, but something before it did not.
    pub dimension: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bound {
    pub via: String,
    pub reason: &'static str,
    pub known: usize,
    pub included: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Exclusion {
    pub via: String,
    pub relation: &'static str,
    pub count: usize,
    pub reason: &'static str,
}

/// No context, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub schema: &'static str,
    /// `invalid_request`, `unavailable`, `stale_state`, `not_found`,
    /// `incomplete_compilation` or `insufficient_budget`.
    pub status: &'static str,
    /// The current state, where the repository could be read.
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub seeds: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
    /// For `insufficient_budget`: what the seeds alone need.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Used>,
}

pub(crate) fn unavailable(problems: Vec<Problem>) -> TaskResult {
    refuse("unavailable", None, Vec::new(), problems)
}

fn refuse(
    status: &'static str,
    state: Option<String>,
    seeds: Vec<String>,
    problems: Vec<Problem>,
) -> TaskResult {
    TaskResult::Refused(Refusal {
        schema: TASK_SCHEMA,
        status,
        state,
        seeds,
        problems,
        required: None,
    })
}

/// The priority classes, in order.
const CLASSES: &[&str] = &[
    "seed",
    "correctness",
    "authority",
    "edit_safety",
    "test",
    "supporting",
    "context",
];

/// The documentation authorities, in priority order, with the class each is in.
const DOC_ORDER: &[(&str, &str)] = &[
    ("normative", "authority"),
    ("grammar", "authority"),
    ("law", "authority"),
    ("interface", "authority"),
    ("architecture", "authority"),
    ("evidence", "authority"),
    ("guide", "supporting"),
    ("operational", "supporting"),
    ("planning", "context"),
    ("research", "context"),
    ("goal", "context"),
];

fn doc_class(authority: &str) -> &'static str {
    DOC_ORDER
        .iter()
        .find(|(a, _)| *a == authority)
        .map_or("context", |(_, c)| c)
}

fn doc_rank(authority: &str) -> usize {
    DOC_ORDER
        .iter()
        .position(|(a, _)| *a == authority)
        .unwrap_or(DOC_ORDER.len())
}

fn class_rank(class: &str) -> usize {
    CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap_or(CLASSES.len())
}

/// A seed, parsed. Every part is an identity compared with the repository's own, never a
/// path that is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seed {
    Package(String),
    Target(String),
    Test(String),
    Sources(String),
    Root(String, String),
    Module(String, String),
    Def(String, String),
    Doc(String),
    Schema(String),
    Profile(String),
    Mutation(String),
    Diagnostic(String, String, String),
}

/// A part of an id that names a file: a plain name, never a path out of its root.
fn plain(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('/')
        && !part.contains('/')
        && !part.contains('\\')
        && part != "."
        && part != ".."
}

fn parse(seed: &str) -> Result<Seed, Problem> {
    let bad = || Problem::new("not_an_entity_id", seed);
    if seed.len() > 1024 || seed.chars().any(char::is_control) {
        return Err(bad());
    }
    let (kind, rest) = seed.split_once(':').ok_or_else(bad)?;
    let two = |r: &str| -> Result<(String, String), Problem> {
        let (a, b) = r.split_once(':').ok_or_else(bad)?;
        if a.is_empty() || b.is_empty() {
            return Err(bad());
        }
        Ok((a.to_owned(), b.to_owned()))
    };
    if rest.is_empty() {
        return Err(bad());
    }
    Ok(match kind {
        "package" => Seed::Package(rest.to_owned()),
        "target" => Seed::Target(seed.to_owned()),
        "test" => Seed::Test(seed.to_owned()),
        "sources" => Seed::Sources(rest.to_owned()),
        "root" => {
            let (s, r) = two(rest)?;
            if !plain(&r) {
                return Err(bad());
            }
            Seed::Root(s, r)
        }
        "module" => {
            let (s, m) = two(rest)?;
            if !plain(&m) {
                return Err(bad());
            }
            Seed::Module(s, m)
        }
        "def" => {
            let (s, k) = two(rest)?;
            let module = k.split_once("::").map_or("", |(m, _)| m);
            if !plain(module) {
                return Err(bad());
            }
            Seed::Def(s, k)
        }
        "doc" => Seed::Doc(rest.to_owned()),
        "schema" => Seed::Schema(rest.to_owned()),
        "profile" => Seed::Profile(rest.to_owned()),
        "mutation" => Seed::Mutation(rest.to_owned()),
        "diagnostic" => {
            let (s, r) = two(rest)?;
            let (root, id) = r.split_once(':').ok_or_else(bad)?;
            if !plain(root) || id.is_empty() {
                return Err(bad());
            }
            Seed::Diagnostic(s, root.to_owned(), id.to_owned())
        }
        _ => return Err(Problem::new("unsupported_kind", seed)),
    })
}

/// One candidate item, before the budget.
#[derive(Debug, Clone)]
struct Candidate {
    item: Item,
    rank: (usize, usize),
}

/// A request's working set: the repository, the compilations opened for it, and what has
/// been selected. Dropped with the request.
struct Planner<'r> {
    repo: &'r Repository,
    intent: Intent,
    opened: Vec<Compilation>,
    candidates: Vec<Candidate>,
    bounded: Vec<Bound>,
    excluded: BTreeSet<Exclusion>,
    problems: Vec<Problem>,
}

pub(crate) fn plan(root: &Path, layout: &Layout, request: &TaskRequest) -> TaskResult {
    let mut problems = Vec::new();
    if request.seeds.is_empty() {
        problems.push(Problem::new("no_seed", ""));
    }
    if request.seeds.len() > MAX_SEEDS {
        problems.push(Problem::new(
            "too_many_seeds",
            request.seeds.len().to_string(),
        ));
    }
    let mut parsed = Vec::new();
    let mut seen = BTreeSet::new();
    for s in &request.seeds {
        if !seen.insert(s.as_str()) {
            problems.push(Problem::new("duplicate_seed", s.clone()));
        }
        match parse(s) {
            Ok(p) => parsed.push((s.clone(), p)),
            Err(p) => problems.push(p),
        }
    }
    if !problems.is_empty() {
        return refuse("invalid_request", None, Vec::new(), problems);
    }
    let repo = match Repository::load(root, layout) {
        Ok(r) => r,
        Err(p) => return unavailable(p),
    };
    if request.state.as_ref().is_some_and(|s| *s != repo.state) {
        return refuse(
            "stale_state",
            Some(repo.state.clone()),
            Vec::new(),
            Vec::new(),
        );
    }
    let intent = request.intent.unwrap_or(Intent::DEFAULT);
    let mut planner = Planner {
        repo: &repo,
        intent,
        opened: Vec::new(),
        candidates: Vec::new(),
        bounded: Vec::new(),
        excluded: BTreeSet::new(),
        problems: Vec::new(),
    };
    let mut missing = Vec::new();
    let mut incomplete = Vec::new();
    let mut seeds = Vec::new();
    for (id, seed) in &parsed {
        match planner.seed(id, seed) {
            Ok(Some(c)) => seeds.push((id.clone(), seed.clone(), c)),
            Ok(None) => missing.push(id.clone()),
            Err(Found::Incomplete) => incomplete.push(id.clone()),
            Err(Found::Broken(p)) => planner.problems.push(p),
        }
    }
    if !planner.problems.is_empty() {
        let mut p = std::mem::take(&mut planner.problems);
        p.sort();
        p.dedup();
        return refuse("unavailable", Some(repo.state.clone()), Vec::new(), p);
    }
    if !missing.is_empty() {
        return refuse("not_found", Some(repo.state.clone()), missing, Vec::new());
    }
    if !incomplete.is_empty() {
        return refuse(
            "incomplete_compilation",
            Some(repo.state.clone()),
            incomplete,
            Vec::new(),
        );
    }
    let mandatory: Vec<Item> = seeds.iter().map(|(_, _, c)| c.item.clone()).collect();
    let seed_ids: BTreeSet<String> = mandatory.iter().map(|i| i.id.clone()).collect();
    for (id, seed, c) in &seeds {
        planner.expand(id, seed, c);
    }
    if !planner.problems.is_empty() {
        let mut p = std::mem::take(&mut planner.problems);
        p.sort();
        p.dedup();
        return refuse("unavailable", Some(repo.state.clone()), Vec::new(), p);
    }

    // The seeds first, whole, or not at all.
    let need = mandatory.iter().fold(Used::default(), add);
    if exceeds(&need, &request.budget).is_some() {
        return TaskResult::Refused(Refusal {
            schema: TASK_SCHEMA,
            status: "insufficient_budget",
            state: Some(repo.state.clone()),
            seeds: Vec::new(),
            problems: Vec::new(),
            required: Some(need),
        });
    }
    // Then every other candidate once — by its best class — in priority order.
    let mut rest: Vec<Candidate> = std::mem::take(&mut planner.candidates)
        .into_iter()
        .filter(|c| !seed_ids.contains(&c.item.id))
        .collect();
    rest.sort_by(|a, b| (a.rank, &a.item.id).cmp(&(b.rank, &b.item.id)));
    let mut once = BTreeSet::new();
    rest.retain(|c| once.insert(c.item.id.clone()));

    let mut used = need;
    let mut items = mandatory;
    let mut omitted = Vec::new();
    let mut stopped: Option<&'static str> = None;
    for c in rest {
        if let Some(_dim) = stopped {
            omitted.push(omission(&c.item, "priority"));
            continue;
        }
        let next = add(used, &c.item);
        if let Some(dim) = exceeds(&next, &request.budget) {
            stopped = Some(dim);
            omitted.push(omission(&c.item, dim));
            continue;
        }
        used = next;
        items.push(c.item);
    }
    let partial = !omitted.is_empty() || !planner.bounded.is_empty();
    TaskResult::Available(Box::new(TaskContext {
        schema: TASK_SCHEMA,
        status: if partial { "partial" } else { "complete" },
        state: repo.state.clone(),
        intent: intent.word(),
        seeds: request.seeds.clone(),
        budget: request.budget,
        used,
        items,
        omitted,
        bounded: planner.bounded,
        excluded: planner.excluded.into_iter().collect(),
    }))
}

fn omission(item: &Item, dimension: &'static str) -> TaskOmission {
    TaskOmission {
        id: item.id.clone(),
        reason: "budget_exhausted",
        via: item.via.clone(),
        class: item.class,
        dimension,
    }
}

fn add(u: Used, i: &Item) -> Used {
    Used {
        entities: u.entities + 1,
        source_bytes: u.source_bytes + i.source_bytes,
        doc_bytes: u.doc_bytes + i.doc_bytes,
    }
}

/// The first dimension `u` does not fit in.
fn exceeds(u: &Used, b: &Budget) -> Option<&'static str> {
    if b.max_entities.is_some_and(|m| u.entities > m) {
        Some("entities")
    } else if b.max_source_bytes.is_some_and(|m| u.source_bytes > m) {
        Some("source_bytes")
    } else if b.max_doc_bytes.is_some_and(|m| u.doc_bytes > m) {
        Some("doc_bytes")
    } else {
        None
    }
}

/// Why a seed has no item.
enum Found {
    /// Its module is only in compilations that needed syntax recovery.
    Incomplete,
    Broken(Problem),
}

/// A durable definition as found: the compilation it is read from, and it.
#[derive(Debug, Clone)]
struct Located {
    compilation: usize,
    definition: DurableDefinition,
}

impl<'r> Planner<'r> {
    fn push(&mut self, item: Item, doc_authority: Option<&str>) {
        let rank = (class_rank(item.class), doc_authority.map_or(0, doc_rank));
        self.candidates.push(Candidate { item, rank });
    }

    /// Open `root` of `source` once per request.
    fn open(&mut self, source: &'static SourceRoot, root: &'static str) -> Result<usize, Problem> {
        if let Some(i) = self
            .opened
            .iter()
            .position(|c| c.source.id == source.id && c.root == root)
        {
            return Ok(i);
        }
        let c = Compilation::open(self.repo, source, root)?;
        self.opened.push(c);
        Ok(self.opened.len() - 1)
    }

    fn compilation(&self, i: usize) -> Option<&Compilation> {
        self.opened.get(i)
    }

    /// The first complete compilation, in manifest order, that loads `module` and has
    /// `identity` — or whether only an incomplete one loads it.
    fn locate(
        &mut self,
        source: &'static SourceRoot,
        module: &str,
        identity: &str,
    ) -> Result<Option<Located>, Found> {
        let mut recovered = false;
        for r in source.roots {
            let i = self.open(source, r).map_err(Found::Broken)?;
            let Some(c) = self.compilation(i) else {
                continue;
            };
            if !c.includes(module) {
                continue;
            }
            if c.definitions.is_none() {
                recovered = true;
                continue;
            }
            if let Some(d) = c.definition(identity) {
                return Ok(Some(Located {
                    compilation: i,
                    definition: d.clone(),
                }));
            }
        }
        if recovered {
            Err(Found::Incomplete)
        } else {
            Ok(None)
        }
    }

    /// The seed's own item, or `None` if the repository has no such entity.
    fn seed(&mut self, id: &str, seed: &Seed) -> Result<Option<Candidate>, Found> {
        let repo = self.repo;
        let seed_item =
            |kind, authority| item(id.to_owned(), kind, "seed", None, "seed", authority);
        let found = match seed {
            Seed::Package(name) => repo.package(name).map(|p| {
                let mut i = seed_item("package", "cargo");
                i.content = Some(serde_json::json!({
                    "manifest": p.manifest,
                    "targets": p.targets.iter().map(|t| t.id(&p.name)).collect::<Vec<_>>(),
                }));
                i
            }),
            Seed::Target(tid) => repo.packages.iter().find_map(|p| {
                p.targets.iter().find(|t| t.id(&p.name) == *tid).map(|t| {
                    let mut i = seed_item("target", "cargo");
                    i.content = Some(serde_json::json!({"kind": t.kind, "path": t.path}));
                    i
                })
            }),
            Seed::Test(tid) => repo.catalogue.test(tid).map(|t| {
                let mut i = seed_item("test", "mutation_catalogue");
                i.content = Some(serde_json::json!({"target": t.target, "name": t.name}));
                i
            }),
            Seed::Sources(s) => repo.source_root(s).map(|s| {
                let mut i = seed_item("sources", "repository_manifest");
                i.content = Some(serde_json::json!({"dir": s.dir, "roots": s.roots}));
                i
            }),
            Seed::Root(s, r) => {
                let Some((source, root)) = repo
                    .source_root(s)
                    .and_then(|x| x.roots.iter().find(|y| **y == r).map(|y| (x, *y)))
                else {
                    return Ok(None);
                };
                let c = self.open(source, root).map_err(Found::Broken)?;
                self.compilation(c).map(|c| {
                    let mut i = seed_item("root", "compiler");
                    i.content = Some(serde_json::json!({
                        "complete": c.definitions.is_some(),
                        "modules": c.modules.len(),
                    }));
                    i.retrieval = Some(retrieval(
                        "nazm.snapshot/2",
                        &[("root", serde_json::Value::from(c.location()))],
                    ));
                    i
                })
            }
            Seed::Module(s, m) => {
                let Some(source) = repo.source_root(s) else {
                    return Ok(None);
                };
                let Some(text) = repo.sources.get(&(source.id, m.clone())) else {
                    return Ok(None);
                };
                let bytes = text.len();
                let mut found = None;
                for r in source.roots {
                    let c = self.open(source, r).map_err(Found::Broken)?;
                    if self.compilation(c).is_some_and(|c| c.includes(m)) {
                        found = Some(c);
                        break;
                    }
                }
                found.and_then(|c| self.compilation(c)).map(|c| {
                    let mut i = seed_item("module", "compiler");
                    i.compilation = Some(c.id());
                    i.content = Some(serde_json::json!({
                        "path": format!("{}/{m}", source.dir),
                        "bytes": bytes,
                    }));
                    i
                })
            }
            Seed::Def(s, k) => {
                let Some(source) = repo.source_root(s) else {
                    return Ok(None);
                };
                let module = k.split_once("::").map_or("", |(m, _)| m);
                if !repo.sources.contains_key(&(source.id, module.to_owned())) {
                    return Ok(None);
                }
                match self.locate(source, module, k)? {
                    None => None,
                    Some(l) => Some(
                        self.definition(&l, id, "seed", None, "seed", true)
                            .map_err(Found::Broken)?,
                    ),
                }
            }
            Seed::Doc(d) => self.section(d, "seed", None, true).map(|(mut i, _)| {
                i.class = "seed";
                i
            }),
            Seed::Schema(n) => repo.schemas.iter().find(|x| x.name == *n).map(|x| {
                let mut i = seed_item("schema", "interface");
                i.content = Some(serde_json::json!({"file": x.file}));
                i
            }),
            Seed::Profile(n) => repo.catalogue.profile(n).map(|p| {
                let mut i = seed_item("profile", "mutation_catalogue");
                i.content = Some(serde_json::json!({"owns": p.owns}));
                i
            }),
            Seed::Mutation(n) => repo
                .catalogue
                .mutations
                .iter()
                .find(|m| m.name == *n)
                .map(|m| {
                    let mut i = seed_item("mutation", "mutation_catalogue");
                    i.content = Some(serde_json::json!({
                        "file": m.file,
                        "killers": m.killers,
                        "profile": m.profile,
                    }));
                    i
                }),
            Seed::Diagnostic(s, r, d) => {
                let Some((source, root)) = repo
                    .source_root(s)
                    .and_then(|x| x.roots.iter().find(|y| **y == r).map(|y| (x, *y)))
                else {
                    return Ok(None);
                };
                let c = self.open(source, root).map_err(Found::Broken)?;
                self.diagnostic(c, d, id)
            }
        };
        Ok(found.map(|item| Candidate { item, rank: (0, 0) }))
    }

    /// A definition's item: its shape always, its exact source when `source` is asked.
    fn definition(
        &self,
        l: &Located,
        id: &str,
        reason: &'static str,
        via: Option<&str>,
        class: &'static str,
        source: bool,
    ) -> Result<Item, Problem> {
        let c = self
            .compilation(l.compilation)
            .ok_or_else(|| Problem::new("context_unavailable", id))?;
        let packet = match c.context(&l.definition) {
            ContextResult::Available(p) => p,
            ContextResult::Unavailable(_) => return Err(Problem::new("context_unavailable", id)),
        };
        let mut i = item(id.to_owned(), "definition", reason, via, class, "compiler");
        i.compilation = Some(c.id());
        i.retrieval = Some(context_retrieval(c, &l.definition));
        let t = &packet.target;
        let mut content = serde_json::json!({
            "kind": t.kind,
            "name": t.name,
            "declaration": {"file": t.source.file, "start": t.source.start, "end": t.source.end},
        });
        if let Some(map) = content.as_object_mut() {
            if let Some(s) = &t.signature {
                map.insert("signature".into(), serde_json::Value::from(s.clone()));
            }
            if let Some(f) = &t.fields {
                map.insert("fields".into(), serde_json::to_value(f).unwrap_or_default());
            }
            if let Some(v) = &t.variants {
                map.insert(
                    "variants".into(),
                    serde_json::to_value(v).unwrap_or_default(),
                );
            }
            if source {
                map.insert(
                    "source".into(),
                    serde_json::Value::from(t.source.text.clone()),
                );
                i.source_bytes = t.source.text.len();
            }
        }
        i.content = Some(content);
        Ok(i)
    }

    fn packet(&self, l: &Located) -> Option<Box<ContextPacket>> {
        match self.compilation(l.compilation)?.context(&l.definition) {
            ContextResult::Available(p) => Some(p),
            ContextResult::Unavailable(_) => None,
        }
    }

    /// An N32 section's item, with its text when `body` is asked, and its authority.
    fn section(
        &self,
        id: &str,
        reason: &'static str,
        via: Option<&str>,
        body: bool,
    ) -> Option<(Item, &'static str)> {
        let nazm_docs::SectionResult::Available(s) = self.repo.corpus.section(id, None) else {
            return None;
        };
        let authority = authority_word(s.authority);
        let kind = if s.parent.is_none() {
            "document"
        } else {
            "section"
        };
        let mut i = item(
            format!("doc:{id}"),
            kind,
            reason,
            via,
            doc_class(authority),
            authority,
        );
        i.retrieval = Some(retrieval(
            "nazm.docs-section/1",
            &[
                ("id", serde_json::Value::from(id)),
                ("state", serde_json::Value::from(self.repo.corpus.state())),
            ],
        ));
        let mut content = serde_json::json!({"title": s.title, "source": {"path": s.source.path, "lines": s.source.lines}});
        if body {
            if let Some(map) = content.as_object_mut() {
                map.insert("body".into(), serde_json::Value::from(s.body.clone()));
            }
            i.doc_bytes = s.body.len();
        }
        i.content = Some(content);
        Some((i, authority))
    }

    fn diagnostic(&mut self, c: usize, wanted: &str, id: &str) -> Option<Item> {
        let comp = self.opened.get_mut(c)?;
        let IndexResult::Available(index) = comp.diagnostics() else {
            return None;
        };
        index.diagnostics.iter().find(|d| d.id == wanted)?;
        let DetailResult::Available(detail) = comp.detail(wanted, &index.state.source) else {
            return None;
        };
        let comp = self.opened.get(c)?;
        let mut i = item(
            id.to_owned(),
            "diagnostic",
            "seed",
            None,
            "seed",
            "compiler",
        );
        i.compilation = Some(comp.id());
        i.retrieval = Some(retrieval(
            "nazm.diagnostic-detail/1",
            &[
                ("root", serde_json::Value::from(comp.location())),
                ("id", serde_json::Value::from(wanted)),
                ("state", serde_json::Value::from(index.state.source.clone())),
            ],
        ));
        let published = serde_json::to_value(&detail.diagnostic).unwrap_or_default();
        i.content = Some(serde_json::json!({
            "code": published.get("code"),
            "severity": published.get("severity"),
            "message": published.get("message"),
            "help": published.get("help"),
            "primary": detail.locations.primary,
            "owner": detail.owner.as_ref().map(|o| format!("def:{}:{}", comp.source.id, o.identity)),
            "fixes": detail.compiler_fixes,
        }));
        Some(i)
    }

    /// Cut `ids` at [`FAN_OUT`], recording the cut.
    fn bound(&mut self, via: &str, reason: &'static str, ids: Vec<String>) -> Vec<String> {
        let known = ids.len();
        if known > FAN_OUT {
            self.bounded.push(Bound {
                via: via.to_owned(),
                reason,
                known,
                included: FAN_OUT,
            });
        }
        ids.into_iter().take(FAN_OUT).collect()
    }

    fn exclude(&mut self, via: &str, relation: &'static str, count: usize, reason: &'static str) {
        if count > 0 {
            self.excluded.insert(Exclusion {
                via: via.to_owned(),
                relation,
                count,
                reason,
            });
        }
    }

    fn docs(&mut self, via: &str, ids: &[String], reason: &'static str) {
        for d in ids {
            if let Some((i, a)) = self.section(d, reason, Some(via), true) {
                self.push(i, Some(a));
            }
        }
    }

    /// Everything one step from a seed that the intent follows.
    fn expand(&mut self, id: &str, seed: &Seed, c: &Candidate) {
        let repo = self.repo;
        let intent = self.intent;
        let edits = intent == Intent::Edit;
        let tests = matches!(intent, Intent::Edit | Intent::Test);
        match seed {
            Seed::Def(s, k) => {
                let Some(source) = repo.source_root(s) else {
                    return;
                };
                let module = k.split_once("::").map_or("", |(m, _)| m).to_owned();
                let Ok(Some(l)) = self.locate(source, &module, k) else {
                    return;
                };
                self.around_definition(id, source, &l, intent);
                if matches!(intent, Intent::Understand | Intent::Edit) {
                    let docs: Vec<String> = source.docs.iter().map(|d| (*d).to_owned()).collect();
                    self.docs(id, &docs, "authoritative_doc");
                }
                if tests {
                    let path = format!("{}/{}", source.dir, l.definition.file);
                    let span = (l.definition.start, l.definition.end);
                    self.tests_of(id, &path, Some(span), Some((source.id, &l.definition.file)));
                }
            }
            Seed::Diagnostic(s, _, _) => {
                let owner = c
                    .item
                    .content
                    .as_ref()
                    .and_then(|v| v.get("owner"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let Some(owner) = owner else {
                    self.exclude(id, "owner", 1, "no_durable_owner");
                    return;
                };
                let Some(source) = repo.source_root(s) else {
                    return;
                };
                let key = owner
                    .split_once(':')
                    .and_then(|(_, r)| r.split_once(':'))
                    .map(|(_, k)| k.to_owned());
                let Some(key) = key else {
                    return;
                };
                let module = key.split_once("::").map_or("", |(m, _)| m).to_owned();
                if let Ok(Some(l)) =
                    self.locate_in(c.item.compilation.as_deref(), source, &module, &key)
                {
                    match self.definition(
                        &l,
                        &owner,
                        "diagnostic_owner",
                        Some(id),
                        "correctness",
                        true,
                    ) {
                        Ok(i) => self.push(i, None),
                        Err(p) => self.problems.push(p),
                    }
                    if intent != Intent::Test {
                        self.types_of(&owner, &l);
                    }
                    if tests {
                        let path = format!("{}/{}", source.dir, l.definition.file);
                        let span = (l.definition.start, l.definition.end);
                        self.tests_of(
                            &owner,
                            &path,
                            Some(span),
                            Some((source.id, &l.definition.file)),
                        );
                    }
                }
            }
            Seed::Doc(d) => self.around_section(id, d),
            Seed::Package(name) => {
                let Some(p) = repo.package(name) else {
                    return;
                };
                if intent != Intent::Test {
                    self.docs(id, &p.docs, "authoritative_doc");
                    let deps: Vec<String> = p
                        .dependencies
                        .iter()
                        .filter(|(k, _)| *k == "depends_on")
                        .map(|(_, n)| format!("package:{n}"))
                        .collect();
                    for d in self.bound(id, "direct_dependency", deps) {
                        let i = item(
                            d,
                            "package",
                            "direct_dependency",
                            Some(id),
                            "correctness",
                            "cargo",
                        );
                        self.push(i, None);
                    }
                }
                if edits {
                    let dependents: Vec<String> = repo
                        .packages
                        .iter()
                        .filter(|q| q.dependencies.iter().any(|(_, n)| *n == p.name))
                        .map(crate::cargo::Package::id)
                        .collect();
                    for d in self.bound(id, "direct_dependent", dependents) {
                        let i = item(
                            d,
                            "package",
                            "direct_dependent",
                            Some(id),
                            "edit_safety",
                            "cargo",
                        );
                        self.push(i, None);
                    }
                }
                if tests {
                    let owned: Vec<String> = p
                        .targets
                        .iter()
                        .filter(|t| matches!(t.kind, "test" | "lib" | "bin"))
                        .map(|t| t.id(&p.name))
                        .collect();
                    for t in self.bound(id, "owned_target", owned) {
                        let i = item(t, "target", "owned_target", Some(id), "test", "cargo");
                        self.push(i, None);
                    }
                    self.tests_of(id, &p.prefix(), None, None);
                }
            }
            Seed::Target(tid) => {
                if let Some(p) = repo
                    .packages
                    .iter()
                    .find(|p| p.targets.iter().any(|t| t.id(&p.name) == *tid))
                {
                    let i = item(
                        p.id(),
                        "package",
                        "owner_package",
                        Some(id),
                        "correctness",
                        "cargo",
                    );
                    self.push(i, None);
                }
                if tests || intent == Intent::Understand {
                    let inside: Vec<String> = repo
                        .catalogue
                        .tests
                        .iter()
                        .filter(|t| t.target == *tid)
                        .map(|t| t.id.clone())
                        .collect();
                    for t in self.bound(id, "contained_test", inside) {
                        let i = item(
                            t,
                            "test",
                            "contained_test",
                            Some(id),
                            "test",
                            "mutation_catalogue",
                        );
                        self.push(i, None);
                    }
                }
            }
            Seed::Test(tid) => {
                if let Some(t) = repo.catalogue.test(tid) {
                    let i = item(
                        t.target.clone(),
                        "target",
                        "owner_target",
                        Some(id),
                        "correctness",
                        "cargo",
                    );
                    self.push(i, None);
                }
                let killed: Vec<String> = repo
                    .catalogue
                    .mutations
                    .iter()
                    .filter(|m| m.killers.iter().any(|k| k == tid))
                    .map(crate::catalogue::Mutation::id)
                    .collect();
                for m in self.bound(id, "killed_mutation", killed) {
                    let i = item(
                        m,
                        "mutation",
                        "killed_mutation",
                        Some(id),
                        "test",
                        "mutation_catalogue",
                    );
                    self.push(i, None);
                }
            }
            Seed::Mutation(n) => {
                let Some(m) = repo.catalogue.mutations.iter().find(|m| m.name == *n) else {
                    return;
                };
                for k in self.bound(id, "killer_test", m.killers.clone()) {
                    let mut i = item(
                        k,
                        "test",
                        "killer_test",
                        Some(id),
                        "test",
                        "mutation_catalogue",
                    );
                    i.content = Some(serde_json::json!({"mutations": [m.name]}));
                    self.push(i, None);
                }
                if let Some(p) = repo.owner(&m.file) {
                    let i = item(
                        p.id(),
                        "package",
                        "owner_package",
                        Some(id),
                        "correctness",
                        "cargo",
                    );
                    self.push(i, None);
                }
                let file = m.file.clone();
                let find = m.find.clone();
                self.mutated_definition(id, &file, &find);
            }
            Seed::Profile(n) => {
                let Some(p) = repo.catalogue.profile(n) else {
                    return;
                };
                for t in self.bound(id, "observed_target", p.targets.clone()) {
                    let i = item(t, "target", "observed_target", Some(id), "test", "cargo");
                    self.push(i, None);
                }
            }
            Seed::Module(s, m) => {
                let Some(source) = repo.source_root(s) else {
                    return;
                };
                let comp = self.opened.iter().position(|c| {
                    c.source.id == source.id && c.includes(m) && c.definitions.is_some()
                });
                if let Some(ci) = comp {
                    let defs: Vec<DurableDefinition> = self
                        .compilation(ci)
                        .and_then(|c| c.definitions.as_ref())
                        .map(|d| {
                            d.definitions
                                .iter()
                                .filter(|x| x.file == *m)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    let ids: Vec<String> = defs
                        .iter()
                        .map(|d| format!("def:{}:{}", source.id, d.identity))
                        .collect();
                    let kept = self.bound(id, "contained_definition", ids);
                    for d in defs
                        .iter()
                        .filter(|d| kept.contains(&format!("def:{}:{}", source.id, d.identity)))
                    {
                        let l = Located {
                            compilation: ci,
                            definition: d.clone(),
                        };
                        let did = format!("def:{}:{}", source.id, d.identity);
                        match self.definition(
                            &l,
                            &did,
                            "contained_definition",
                            Some(id),
                            "correctness",
                            false,
                        ) {
                            Ok(i) => self.push(i, None),
                            Err(p) => self.problems.push(p),
                        }
                    }
                }
                if intent != Intent::Test {
                    let docs: Vec<String> = source.docs.iter().map(|d| (*d).to_owned()).collect();
                    self.docs(id, &docs, "authoritative_doc");
                }
                if tests {
                    self.tests_of(id, &format!("{}/{m}", source.dir), None, None);
                }
            }
            Seed::Root(s, r) => {
                let Some(source) = repo.source_root(s) else {
                    return;
                };
                if let Some(c) = self
                    .opened
                    .iter()
                    .find(|c| c.source.id == source.id && c.root == r)
                {
                    let modules: Vec<String> = c
                        .modules
                        .iter()
                        .map(|m| format!("module:{}:{m}", source.id))
                        .collect();
                    for m in self.bound(id, "included_module", modules) {
                        let i = item(
                            m,
                            "module",
                            "included_module",
                            Some(id),
                            "correctness",
                            "compiler",
                        );
                        self.push(i, None);
                    }
                }
                if intent != Intent::Test {
                    let docs: Vec<String> = source.docs.iter().map(|d| (*d).to_owned()).collect();
                    self.docs(id, &docs, "authoritative_doc");
                }
                if tests {
                    self.tests_of(id, &format!("{}/{r}", source.dir), None, None);
                }
            }
            Seed::Sources(s) => {
                let Some(source) = repo.source_root(s) else {
                    return;
                };
                let roots: Vec<String> = source
                    .roots
                    .iter()
                    .map(|r| format!("root:{}:{r}", source.id))
                    .collect();
                for r in self.bound(id, "contained_root", roots) {
                    let i = item(
                        r,
                        "root",
                        "contained_root",
                        Some(id),
                        "correctness",
                        "compiler",
                    );
                    self.push(i, None);
                }
                let docs: Vec<String> = source.docs.iter().map(|d| (*d).to_owned()).collect();
                self.docs(id, &docs, "authoritative_doc");
                if tests {
                    self.tests_of(id, &format!("{}/", source.dir), None, None);
                }
            }
            Seed::Schema(_) => {}
        }
    }

    /// Like [`Self::locate`], preferring the compilation the seed was read from.
    fn locate_in(
        &mut self,
        compilation: Option<&str>,
        source: &'static SourceRoot,
        module: &str,
        identity: &str,
    ) -> Result<Option<Located>, Found> {
        if let Some(i) = self
            .opened
            .iter()
            .position(|c| Some(c.id().as_str()) == compilation)
        {
            if let Some(d) = self.compilation(i).and_then(|c| c.definition(identity)) {
                return Ok(Some(Located {
                    compilation: i,
                    definition: d.clone(),
                }));
            }
        }
        self.locate(source, module, identity)
    }

    /// The seed definition's direct links: what its signature names and what it uses, and —
    /// for an edit — what uses it, in every compilation of the manifest that loads it.
    fn around_definition(
        &mut self,
        id: &str,
        source: &'static SourceRoot,
        l: &Located,
        intent: Intent,
    ) {
        let Some(packet) = self.packet(l) else {
            self.problems.push(Problem::new("context_unavailable", id));
            return;
        };
        if intent == Intent::Test {
            return;
        }
        self.types_of(id, l);
        if intent == Intent::Diagnose {
            self.diagnostics_in(id, l);
            return;
        }
        let this = l.definition.identity.clone();
        // What its signature names is already selected, by the more specific reason.
        let typed: BTreeSet<String> = packet
            .related_types
            .iter()
            .filter_map(|d| d.identity.definition.clone())
            .collect();
        let mut deps: Vec<String> = packet
            .dependencies
            .iter()
            .filter_map(|d| d.identity.definition.clone())
            .filter(|k| *k != this && !typed.contains(k))
            .collect();
        deps.sort();
        deps.dedup();
        let untracked = packet
            .dependencies
            .iter()
            .filter(|d| d.identity.definition.is_none())
            .count();
        self.exclude(id, "dependency", untracked, "no_durable_identity");
        let keep = self.bound(id, "direct_dependency", deps);
        for k in keep {
            self.linked(id, l.compilation, &k, "direct_dependency", "correctness");
        }
        let calls = packet.callers.items.as_ref().map_or(0, Vec::len);
        if intent == Intent::Edit {
            self.users(id, source, l);
        } else {
            self.exclude(id, "direct_caller", calls, "not_selected_for_intent");
            self.exclude(
                id,
                "reference",
                packet.references.len(),
                "not_selected_for_intent",
            );
        }
    }

    /// The records and enums `l`'s declaration names, by shape.
    fn types_of(&mut self, via: &str, l: &Located) {
        let Some(packet) = self.packet(l) else {
            return;
        };
        let this = l.definition.identity.clone();
        let mut types: Vec<String> = packet
            .related_types
            .iter()
            .filter_map(|d| d.identity.definition.clone())
            .filter(|k| *k != this)
            .collect();
        types.sort();
        types.dedup();
        for k in self.bound(via, "signature_type", types) {
            self.linked(via, l.compilation, &k, "signature_type", "correctness");
        }
    }

    /// A linked definition, by shape, from the compilation the link was read in.
    fn linked(
        &mut self,
        via: &str,
        compilation: usize,
        key: &str,
        reason: &'static str,
        class: &'static str,
    ) {
        let Some(c) = self.compilation(compilation) else {
            return;
        };
        let Some(d) = c.definition(key).cloned() else {
            self.exclude(via, reason, 1, "no_durable_identity");
            return;
        };
        let did = format!("def:{}:{key}", c.source.id);
        let l = Located {
            compilation,
            definition: d,
        };
        match self.definition(&l, &did, reason, Some(via), class, false) {
            Ok(i) => self.push(i, None),
            Err(p) => self.problems.push(p),
        }
    }

    /// The diagnostics inside `l`'s declaration, by N30 id.
    fn diagnostics_in(&mut self, via: &str, l: &Located) {
        let Some(c) = self.compilation(l.compilation) else {
            return;
        };
        let IndexResult::Available(index) = c.diagnostics() else {
            return;
        };
        let mut found = Vec::new();
        for d in &index.diagnostics {
            if d.owner
                .as_ref()
                .is_some_and(|o| o.identity == l.definition.identity)
            {
                let mut i = item(
                    format!("diagnostic:{}:{}:{}", c.source.id, c.root, d.id),
                    "diagnostic",
                    "diagnostic_in_seed",
                    Some(via),
                    "correctness",
                    "compiler",
                );
                i.compilation = Some(c.id());
                i.retrieval = Some(retrieval(
                    "nazm.diagnostic-detail/1",
                    &[
                        ("root", serde_json::Value::from(c.location())),
                        ("id", serde_json::Value::from(d.id.clone())),
                        ("state", serde_json::Value::from(index.state.source.clone())),
                    ],
                ));
                i.content = Some(
                    serde_json::json!({"code": d.code, "severity": d.severity, "primary": d.primary}),
                );
                found.push(i);
            }
        }
        let ids: Vec<String> = found.iter().map(|i| i.id.clone()).collect();
        let keep = self.bound(via, "diagnostic_in_seed", ids);
        for i in found.into_iter().filter(|i| keep.contains(&i.id)) {
            self.push(i, None);
        }
    }

    /// What uses the seed, in every compilation of its source root that loads its module:
    /// a function's direct callers (N25), a type's using definitions (N17).
    fn users(&mut self, id: &str, source: &'static SourceRoot, l: &Located) {
        let module = l.definition.file.clone();
        let identity = l.definition.identity.clone();
        let mut users: std::collections::BTreeMap<String, (usize, Vec<serde_json::Value>)> =
            std::collections::BTreeMap::new();
        let function = l.definition.kind == "function";
        for r in source.roots {
            let i = match self.open(source, r) {
                Ok(i) => i,
                Err(p) => {
                    self.problems.push(p);
                    continue;
                }
            };
            let Some(c) = self.compilation(i) else {
                continue;
            };
            if !c.includes(&module) {
                continue;
            }
            let Some(d) = c.definition(&identity).cloned() else {
                continue;
            };
            let ContextResult::Available(p) = c.context(&d) else {
                continue;
            };
            if function {
                for call in p.callers.items.iter().flatten() {
                    let Some(k) = &call.identity.definition else {
                        continue;
                    };
                    if *k == identity {
                        continue;
                    }
                    let e = users.entry(k.clone()).or_insert_with(|| (i, Vec::new()));
                    for s in &call.calls {
                        e.1.push(
                            serde_json::json!({"file": s.file, "start": s.start, "end": s.end}),
                        );
                    }
                }
            } else if let Some(defs) = &c.definitions {
                for u in &p.references {
                    let owner = defs.definitions.iter().find(|x| {
                        x.file == u.file
                            && x.start <= u.start
                            && u.end <= x.end
                            && x.identity != identity
                    });
                    if let Some(o) = owner {
                        let e = users
                            .entry(o.identity.clone())
                            .or_insert_with(|| (i, Vec::new()));
                        e.1.push(
                            serde_json::json!({"file": u.file, "start": u.start, "end": u.end}),
                        );
                    }
                }
            }
        }
        let reason = if function {
            "direct_caller"
        } else {
            "direct_reference"
        };
        let ids: Vec<String> = users.keys().cloned().collect();
        let keep: BTreeSet<String> = self.bound(id, reason, ids).into_iter().collect();
        for (k, (ci, mut spans)) in users {
            if !keep.contains(&k) {
                continue;
            }
            spans.sort_by_key(|v| {
                (
                    v.get("file")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    v.get("start").and_then(serde_json::Value::as_u64),
                )
            });
            spans.dedup();
            let Some(c) = self.compilation(ci) else {
                continue;
            };
            let Some(d) = c.definition(&k).cloned() else {
                continue;
            };
            let mut i = item(
                format!("def:{}:{k}", source.id),
                "definition",
                reason,
                Some(id),
                "edit_safety",
                "compiler",
            );
            i.compilation = Some(c.id());
            i.retrieval = Some(context_retrieval(c, &d));
            i.content = Some(serde_json::json!({"kind": d.kind, "uses": spans}));
            self.push(i, None);
        }
    }

    /// The seed section's direct links, and for an edit the sections that link to it.
    fn around_section(&mut self, id: &str, d: &str) {
        let nazm_docs::SectionResult::Available(s) = self.repo.corpus.section(d, None) else {
            return;
        };
        let mut linked: Vec<(&'static str, String)> = s
            .links
            .iter()
            .filter_map(|l| {
                let reason = match l.relation {
                    "generated_from" => "generated_from",
                    "references" => "grammar_reference",
                    _ => "linked_section",
                };
                l.target.clone().map(|t| (reason, t))
            })
            .filter(|(_, t)| t != d)
            .collect();
        linked.sort();
        linked.dedup();
        for reason in ["generated_from", "grammar_reference", "linked_section"] {
            let ids: Vec<String> = linked
                .iter()
                .filter(|(r, _)| *r == reason)
                .map(|(_, t)| t.clone())
                .collect();
            for t in self.bound(id, reason, ids) {
                if let Some((i, a)) = self.section(&t, reason, Some(id), true) {
                    self.push(i, Some(a));
                }
            }
        }
        if self.intent == Intent::Understand {
            let children = s.children.clone();
            for t in self.bound(id, "child_section", children) {
                if let Some((mut i, a)) = self.section(&t, "child_section", Some(id), false) {
                    i.class = "supporting";
                    self.push(i, Some(a));
                }
            }
        }
        if self.intent == Intent::Edit {
            let nazm_docs::IndexResult::Available(index) = self.repo.corpus.index(None) else {
                return;
            };
            let mut incoming = Vec::new();
            for doc in &index.documents {
                for sec in &doc.sections {
                    if sec.id == d {
                        continue;
                    }
                    if let nazm_docs::SectionResult::Available(x) =
                        self.repo.corpus.section(&sec.id, None)
                    {
                        if x.links.iter().any(|l| l.target.as_deref() == Some(d)) {
                            incoming.push(sec.id.clone());
                        }
                    }
                }
            }
            for t in self.bound(id, "linking_section", incoming) {
                if let Some((mut i, a)) = self.section(&t, "linking_section", Some(id), false) {
                    i.class = "edit_safety";
                    self.push(i, Some(a));
                }
            }
        }
    }

    /// The tests the catalogue relates to `path` — to the definition spanning `span` of it,
    /// when there is one: killers of each mutation located there, and the targets of the
    /// profile that owns the path.
    fn tests_of(
        &mut self,
        via: &str,
        path: &str,
        span: Option<(u32, u32)>,
        text: Option<(&str, &str)>,
    ) {
        let repo = self.repo;
        let source = text.and_then(|(s, f)| repo.sources.get(&(s, f.to_owned())));
        let mut killers: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for m in &repo.catalogue.mutations {
            let inside = match span {
                None => m.file.starts_with(path),
                Some((start, end)) => {
                    m.file == path
                        && source.is_some_and(|t| {
                            located(t, &m.find).is_some_and(|(a, b)| a >= start && b <= end)
                        })
                }
            };
            if inside {
                for k in &m.killers {
                    killers.entry(k.clone()).or_default().push(m.name.clone());
                }
            }
        }
        let ids: Vec<String> = killers.keys().cloned().collect();
        let keep: BTreeSet<String> = self.bound(via, "killer_test", ids).into_iter().collect();
        for (k, mutations) in killers {
            if !keep.contains(&k) {
                continue;
            }
            let target = repo.catalogue.test(&k).map(|t| t.target.clone());
            let mut i = item(
                k,
                "test",
                "killer_test",
                Some(via),
                "test",
                "mutation_catalogue",
            );
            i.content = Some(serde_json::json!({"target": target, "mutations": mutations}));
            self.push(i, None);
        }
        if let Some(p) = repo.catalogue.profile_for(path) {
            let name = p.name.clone();
            for t in self.bound(via, "profile_target", p.targets.clone()) {
                let mut i = item(
                    t,
                    "target",
                    "profile_target",
                    Some(via),
                    "test",
                    "mutation_catalogue",
                );
                i.content = Some(serde_json::json!({"profile": name}));
                self.push(i, None);
            }
        }
    }

    /// The durable definition a mutation's `find` is inside, where its file is a module.
    fn mutated_definition(&mut self, via: &str, file: &str, find: &str) {
        let repo = self.repo;
        let Some(source) = repo.layout.sources.iter().find(|s| {
            file.strip_prefix(s.dir)
                .and_then(|r| r.strip_prefix('/'))
                .is_some_and(plain)
        }) else {
            return;
        };
        let module = file
            .get(source.dir.len() + 1..)
            .unwrap_or_default()
            .to_owned();
        let Some(text) = repo.sources.get(&(source.id, module.clone())) else {
            return;
        };
        let Some((a, b)) = located(text, find) else {
            return;
        };
        for r in source.roots {
            let i = match self.open(source, r) {
                Ok(i) => i,
                Err(p) => {
                    self.problems.push(p);
                    continue;
                }
            };
            let Some(c) = self.compilation(i) else {
                continue;
            };
            let Some(defs) = &c.definitions else {
                continue;
            };
            if let Some(d) = defs
                .definitions
                .iter()
                .find(|d| d.file == module && d.start <= a && b <= d.end)
                .cloned()
            {
                let did = format!("def:{}:{}", source.id, d.identity);
                let l = Located {
                    compilation: i,
                    definition: d,
                };
                match self.definition(
                    &l,
                    &did,
                    "mutated_definition",
                    Some(via),
                    "correctness",
                    true,
                ) {
                    Ok(item) => self.push(item, None),
                    Err(p) => self.problems.push(p),
                }
                return;
            }
        }
    }
}

fn item(
    id: String,
    kind: &'static str,
    reason: &'static str,
    via: Option<&str>,
    class: &'static str,
    authority: &'static str,
) -> Item {
    Item {
        id,
        kind,
        reason,
        via: via.map(str::to_owned),
        class,
        authority,
        compilation: None,
        retrieval: None,
        content: None,
        source_bytes: 0,
        doc_bytes: 0,
    }
}

/// Where `find` is in `text`, when it is there exactly once — the catalogue's own rule for
/// where a mutation lands.
fn located(text: &str, find: &str) -> Option<(u32, u32)> {
    let mut at = text.match_indices(find);
    let (start, _) = at.next()?;
    if at.next().is_some() {
        return None;
    }
    let start = u32::try_from(start).ok()?;
    let end = u32::try_from(find.len()).ok()?.checked_add(start)?;
    Some((start, end))
}

fn context_retrieval(c: &Compilation, d: &DurableDefinition) -> Retrieval {
    retrieval(
        "nazm.context/2",
        &[
            ("root", serde_json::Value::from(c.location())),
            ("file", serde_json::Value::from(d.file.clone())),
            ("byte", serde_json::Value::from(d.name_start)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{parse, Seed};

    #[test]
    fn a_path_is_never_a_seed() {
        for bad in [
            "/etc/passwd",
            "../../etc/passwd",
            "module:compiler:../lex.nz",
            "module:compiler:/etc/passwd",
            "def:compiler:../x.nz::fn f",
            "root:compiler:sub/emit.nz",
            "diagnostic:examples:../x.nz:N0300@x",
            "doc",
            "package:",
            "def:compiler:",
            "module:compiler:a\u{0}.nz",
        ] {
            assert!(parse(bad).is_err(), "{bad} parsed");
        }
        assert_eq!(
            parse("def:compiler:lex.nz::fn next").ok(),
            Some(Seed::Def("compiler".into(), "lex.nz::fn next".into()))
        );
        assert_eq!(
            parse("diagnostic:examples-bad:r.nz:N0300@r.nz:59-69#0").ok(),
            Some(Seed::Diagnostic(
                "examples-bad".into(),
                "r.nz".into(),
                "N0300@r.nz:59-69#0".into()
            ))
        );
        assert!(matches!(parse("glob:*"), Err(p) if p.kind == "unsupported_kind"));
    }

    #[test]
    fn a_mutation_lands_only_where_it_occurs_once() {
        assert_eq!(super::located("abcabc", "bc"), None);
        assert_eq!(super::located("abcdef", "cd"), Some((2, 4)));
        assert_eq!(super::located("abc", "x"), None);
    }
}
