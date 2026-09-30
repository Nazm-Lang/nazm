//! `nazm.repository-map/1`: what the repository has, how it is related, and where to ask
//! for more — identities and structure, never a body.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::repository::Repository;
use crate::sources::Compilation;
use crate::{Layout, Problem, COMPILER, MAP_SCHEMA};

/// A repository map, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MapResult {
    Available(Box<RepositoryMap>),
    Unavailable(Unavailable),
}

/// Why there is no map: every problem, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    pub schema: &'static str,
    pub status: &'static str,
    pub problems: Vec<Problem>,
}

/// The map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryMap {
    pub schema: &'static str,
    pub status: &'static str,
    /// The repository state every id here is of.
    pub state: String,
    pub compiler: &'static str,
    /// Each entity kind: its authority, what its identity is, and how many there are.
    pub classes: Vec<Class>,
    /// In canonical order: by kind, in [`KINDS`] order, then by id.
    pub entities: Vec<Entity>,
    /// Entities that exist and are enumerated by the subsystem that owns them, not here.
    pub delegated: Vec<Delegated>,
    /// What has no durable identity, and so is not an entity.
    pub omitted: Vec<Omission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Class {
    pub kind: &'static str,
    /// The authority every entity of the kind is read from; a document's own authority,
    /// where it has one, is on the entity.
    pub authority: &'static str,
    pub identity: &'static str,
    pub count: usize,
}

/// One entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entity {
    pub id: String,
    pub kind: &'static str,
    /// A target's Cargo kind, a definition's kind, or whether a compilation is complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'static str>,
    /// Only where it is not the class's: a document's N32 authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Where it is, from the repository root. A location, never its identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// How many current diagnostics a compilation has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<usize>,
    /// By kind, then target.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub retrieval: Vec<Retrieval>,
}

/// A structural relationship, with the entity (or delegated N32 section) it points at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Relation {
    pub kind: &'static str,
    pub target: String,
}

/// Which existing answer holds more, and exactly how to ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retrieval {
    pub schema: &'static str,
    pub args: BTreeMap<&'static str, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delegated {
    pub kind: &'static str,
    pub authority: &'static str,
    pub count: usize,
    pub retrieval: Retrieval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Omission {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    pub reason: &'static str,
}

/// Every entity kind, in canonical order, with its authority and its identity.
pub(crate) const KINDS: &[(&str, &str, &str)] = &[
    ("package", "cargo", "the Cargo package name"),
    (
        "target",
        "cargo",
        "package, Cargo target kind and target name",
    ),
    (
        "sources",
        "repository_manifest",
        "the source root's manifest id",
    ),
    (
        "root",
        "compiler",
        "source root and the root module's durable key",
    ),
    ("module", "compiler", "source root and durable module key"),
    (
        "definition",
        "compiler",
        "source root and durable definition key",
    ),
    ("document", "documentation", "the N32 document id"),
    (
        "schema",
        "interface",
        "the schema constant the file declares",
    ),
    ("profile", "mutation_catalogue", "the profile name"),
];

/// Every relationship kind a map may carry.
pub(crate) const RELATIONS: &[&str] = &[
    "covers",
    "depends_on",
    "dev_depends_on",
    "documented_by",
    "includes",
    "observed_by",
];

/// Every schema a retrieval may name.
pub(crate) const RETRIEVALS: &[&str] = &[
    "nazm.context/3",
    "nazm.diagnostic-detail/1",
    "nazm.diagnostic-index/1",
    "nazm.docs-index/1",
    "nazm.docs-section/1",
    "nazm.snapshot/3",
    "nazm.task-context/1",
];

/// Every authority an entity may carry: N32's, and the non-document ones above.
pub(crate) const AUTHORITIES: &[&str] = &[
    "architecture",
    "cargo",
    "compiler",
    "documentation",
    "evidence",
    "goal",
    "grammar",
    "guide",
    "interface",
    "law",
    "mutation_catalogue",
    "normative",
    "operational",
    "planning",
    "repository_manifest",
    "research",
];

pub(crate) fn unavailable(problems: Vec<Problem>) -> MapResult {
    MapResult::Unavailable(Unavailable {
        schema: MAP_SCHEMA,
        status: "unavailable",
        problems,
    })
}

pub(crate) fn retrieval(
    schema: &'static str,
    args: &[(&'static str, serde_json::Value)],
) -> Retrieval {
    Retrieval {
        schema,
        args: args.iter().cloned().collect(),
    }
}

/// An N32 authority, as the word the documentation index writes.
pub(crate) fn authority_word(a: nazm_docs::Authority) -> &'static str {
    use nazm_docs::Authority as A;
    match a {
        A::Goal => "goal",
        A::Law => "law",
        A::Architecture => "architecture",
        A::Normative => "normative",
        A::Grammar => "grammar",
        A::Guide => "guide",
        A::Interface => "interface",
        A::Evidence => "evidence",
        A::Planning => "planning",
        A::Research => "research",
        A::Operational => "operational",
    }
}

/// A durable definition as first found: its kind, its file, and its declaration's place,
/// which every other compilation must agree with.
type Found = (&'static str, String, (String, u32, u32));

fn entity(id: String, kind: &'static str) -> Entity {
    Entity {
        id,
        kind,
        detail: None,
        authority: None,
        parent: None,
        source: None,
        diagnostics: None,
        relations: Vec::new(),
        retrieval: Vec::new(),
    }
}

pub(crate) fn build(root: &Path, layout: &Layout) -> MapResult {
    let repo = match Repository::load(root, layout) {
        Ok(r) => r,
        Err(p) => return unavailable(p),
    };
    let mut problems = Vec::new();
    let mut entities = Vec::new();

    for p in &repo.packages {
        let mut e = entity(p.id(), "package");
        e.source = Some(p.manifest.clone());
        for (kind, name) in &p.dependencies {
            e.relations.push(Relation {
                kind,
                target: format!("package:{name}"),
            });
        }
        for d in &p.docs {
            e.relations.push(Relation {
                kind: "documented_by",
                target: format!("doc:{d}"),
            });
        }
        entities.push(e);
        for t in &p.targets {
            let mut e = entity(t.id(&p.name), "target");
            e.detail = Some(t.kind);
            e.parent = Some(p.id());
            e.source = Some(t.path.clone());
            entities.push(e);
        }
    }

    // Every compilation, in manifest order; a definition is its first complete
    // compilation's, and must be the same declaration in every other.
    let mut definitions: BTreeMap<(String, String), Found> = BTreeMap::new();
    let mut modules: BTreeSet<(String, String)> = BTreeSet::new();
    let (mut diagnostics, mut recovered, mut untracked) = (0, 0, 0);
    for s in layout.sources {
        let mut e = entity(format!("sources:{}", s.id), "sources");
        e.source = Some(s.dir.to_owned());
        for d in s.docs {
            e.relations.push(Relation {
                kind: "documented_by",
                target: format!("doc:{d}"),
            });
        }
        entities.push(e);
        for r in s.roots {
            let c = match Compilation::open(&repo, s, r) {
                Ok(c) => c,
                Err(p) => {
                    problems.push(p);
                    continue;
                }
            };
            let mut e = entity(c.id(), "root");
            e.parent = Some(format!("sources:{}", s.id));
            e.source = Some(c.location());
            e.detail = Some(if c.definitions.is_some() {
                "complete"
            } else {
                "incomplete"
            });
            let count = match c.diagnostics() {
                nazm_service::IndexResult::Available(i) => i.counts.total,
                nazm_service::IndexResult::Unavailable(_) => 0,
            };
            diagnostics += count;
            e.diagnostics = Some(count);
            for m in &c.modules {
                modules.insert((s.id.to_owned(), m.clone()));
                e.relations.push(Relation {
                    kind: "includes",
                    target: format!("module:{}:{m}", s.id),
                });
            }
            let at = serde_json::Value::from(c.location());
            e.retrieval
                .push(retrieval("nazm.snapshot/3", &[("root", at.clone())]));
            e.retrieval
                .push(retrieval("nazm.diagnostic-index/1", &[("root", at)]));
            entities.push(e);
            match &c.definitions {
                None => recovered += 1,
                Some(d) => {
                    untracked += d.untracked;
                    for def in &d.definitions {
                        let place = (def.file.clone(), def.start, def.end);
                        let key = (s.id.to_owned(), def.identity.clone());
                        match definitions.get(&key) {
                            Some((_, _, p)) if *p != place => problems.push(Problem::new(
                                "conflicting_definition",
                                format!("def:{}:{}", s.id, def.identity),
                            )),
                            Some(_) => {}
                            None => {
                                definitions.insert(key, (def.kind, def.file.clone(), place));
                            }
                        }
                    }
                }
            }
        }
    }
    for (s, m) in &modules {
        let dir = layout
            .sources
            .iter()
            .find(|x| x.id == s)
            .map_or("", |x| x.dir);
        let mut e = entity(format!("module:{s}:{m}"), "module");
        e.parent = Some(format!("sources:{s}"));
        e.source = Some(format!("{dir}/{m}"));
        entities.push(e);
    }
    for ((s, identity), (kind, file, _)) in &definitions {
        let mut e = entity(format!("def:{s}:{identity}"), "definition");
        e.detail = Some(kind);
        e.parent = Some(format!("module:{s}:{file}"));
        entities.push(e);
    }

    let index = repo.corpus.index(None);
    let nazm_docs::IndexResult::Available(index) = index else {
        return unavailable(vec![Problem::new("documentation_unavailable", "index")]);
    };
    for d in &index.documents {
        let mut e = entity(format!("doc:{}", d.id), "document");
        e.authority = Some(authority_word(d.authority));
        e.source = Some(d.path.to_owned());
        e.retrieval.push(retrieval(
            "nazm.docs-index/1",
            &[("document", serde_json::Value::from(d.id))],
        ));
        entities.push(e);
    }
    for s in &repo.schemas {
        let mut e = entity(format!("schema:{}", s.name), "schema");
        e.source = Some(s.file.clone());
        entities.push(e);
    }
    for p in &repo.catalogue.profiles {
        let mut e = entity(p.id(), "profile");
        e.source = layout.catalogue.map(str::to_owned);
        for t in &p.targets {
            e.relations.push(Relation {
                kind: "observed_by",
                target: t.clone(),
            });
        }
        for o in &p.owns {
            let covered = repo
                .packages
                .iter()
                .filter(|q| !q.dir.is_empty() && q.prefix() == *o)
                .map(crate::cargo::Package::id)
                .chain(
                    layout
                        .sources
                        .iter()
                        .filter(|s| format!("{}/", s.dir) == *o)
                        .map(|s| format!("sources:{}", s.id)),
                );
            for target in covered {
                e.relations.push(Relation {
                    kind: "covers",
                    target,
                });
            }
        }
        entities.push(e);
    }
    if !problems.is_empty() {
        problems.sort();
        problems.dedup();
        return unavailable(problems);
    }

    canonical(&mut entities);
    let classes = KINDS
        .iter()
        .map(|&(kind, authority, identity)| Class {
            kind,
            authority,
            identity,
            count: entities.iter().filter(|e| e.kind == kind).count(),
        })
        .collect();
    let counts = repo.corpus.counts();
    let catalogue = layout.catalogue.unwrap_or_default();
    let map = RepositoryMap {
        schema: MAP_SCHEMA,
        status: "available",
        state: repo.state.clone(),
        compiler: COMPILER,
        classes,
        entities,
        delegated: vec![
            Delegated {
                kind: "section",
                authority: "documentation",
                count: counts.sections.saturating_sub(counts.documents),
                retrieval: retrieval("nazm.docs-index/1", &[]),
            },
            Delegated {
                kind: "mutation",
                authority: "mutation_catalogue",
                count: repo.catalogue.mutations.len(),
                retrieval: retrieval(
                    "nazm.task-context/1",
                    &[("catalogue", serde_json::Value::from(catalogue))],
                ),
            },
            Delegated {
                kind: "test",
                authority: "mutation_catalogue",
                count: repo.catalogue.tests.len(),
                retrieval: retrieval(
                    "nazm.task-context/1",
                    &[("catalogue", serde_json::Value::from(catalogue))],
                ),
            },
            Delegated {
                kind: "diagnostic",
                authority: "compiler",
                count: diagnostics,
                retrieval: retrieval("nazm.diagnostic-index/1", &[]),
            },
        ],
        omitted: vec![
            Omission {
                kind: "rust_item",
                count: None,
                reason: "no compiler-owned semantic identity for Rust source: its packages and targets are mapped",
            },
            Omission {
                kind: "local",
                count: None,
                reason: "a local, parameter or label has no durable identity",
            },
            Omission {
                kind: "prelude_definition",
                count: None,
                reason: "the core prelude has no source file in any compilation",
            },
            Omission {
                kind: "untracked_definition",
                count: Some(untracked),
                reason: "no durable key, or one key given to two declarations",
            },
            Omission {
                kind: "recovered_compilation",
                count: Some(recovered),
                reason: "needed syntax recovery or lacks a module: its modules are mapped, its definitions are not",
            },
            Omission {
                kind: "test_function",
                count: None,
                reason: "a test the catalogue does not name as a killer has no identity without building and listing it",
            },
        ],
    };
    let problems = validate(&map, &|id| {
        id.strip_prefix("doc:").is_some_and(|s| {
            matches!(
                repo.corpus.section(s, None),
                nazm_docs::SectionResult::Available(_)
            )
        })
    });
    if problems.is_empty() {
        MapResult::Available(Box::new(map))
    } else {
        unavailable(problems)
    }
}

/// The canonical order: kinds in [`KINDS`] order, ids in byte order; each entity's
/// relations by kind and target, once.
pub(crate) fn canonical(entities: &mut [Entity]) {
    for e in entities.iter_mut() {
        e.relations.sort();
        e.relations.dedup();
    }
    entities.sort_by(|a, b| (rank(a.kind), &a.id).cmp(&(rank(b.kind), &b.id)));
}

fn rank(kind: &str) -> usize {
    KINDS
        .iter()
        .position(|(k, _, _)| *k == kind)
        .unwrap_or(KINDS.len())
}

/// Every way `map` is not a map this crate may publish: a duplicate id, a parent or a
/// relationship target that does not exist — an N32 section counts as existing when
/// `section` says so — an unknown kind, relationship, retrieval or authority, or an order
/// that is not canonical.
pub fn validate(map: &RepositoryMap, section: &dyn Fn(&str) -> bool) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut ids = BTreeSet::new();
    for e in &map.entities {
        if !ids.insert(e.id.as_str()) {
            problems.push(Problem::new("duplicate_entity", e.id.clone()));
        }
        if rank(e.kind) == KINDS.len() {
            problems.push(Problem::new("unknown_kind", e.id.clone()));
        }
        if !e.id.starts_with(&format!("{}:", prefix(e.kind))) {
            problems.push(Problem::new("malformed_id", e.id.clone()));
        }
    }
    for e in &map.entities {
        if let Some(p) = &e.parent {
            if !ids.contains(p.as_str()) {
                problems.push(Problem::new("broken_parent", e.id.clone()));
            }
        }
        for r in &e.relations {
            if !RELATIONS.contains(&r.kind) {
                problems.push(Problem::new("unknown_relation", e.id.clone()));
            }
            let found = ids.contains(r.target.as_str())
                || (r.kind == "documented_by" && section(&r.target));
            if !found {
                problems.push(Problem::new(
                    "broken_relation",
                    format!("{} {} {}", e.id, r.kind, r.target),
                ));
            }
        }
        if e.relations
            .windows(2)
            .any(|w| matches!(w, [a, b] if a >= b))
        {
            problems.push(Problem::new("unordered_relations", e.id.clone()));
        }
        for r in &e.retrieval {
            if !RETRIEVALS.contains(&r.schema) {
                problems.push(Problem::new("unknown_retrieval", e.id.clone()));
            }
        }
        if let Some(a) = e.authority {
            if !AUTHORITIES.contains(&a) {
                problems.push(Problem::new("unknown_authority", e.id.clone()));
            }
        }
        if e.kind == "document" && e.authority.is_none() {
            problems.push(Problem::new("missing_authority", e.id.clone()));
        }
    }
    let order: Vec<(usize, &str)> = map
        .entities
        .iter()
        .map(|e| (rank(e.kind), e.id.as_str()))
        .collect();
    if order.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
        problems.push(Problem::new("unordered_entities", ""));
    }
    for c in &map.classes {
        if rank(c.kind) == KINDS.len() {
            problems.push(Problem::new("unknown_kind", c.kind));
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

/// The id prefix of a kind.
pub(crate) fn prefix(kind: &str) -> &str {
    match kind {
        "definition" => "def",
        "document" => "doc",
        other => other,
    }
}
