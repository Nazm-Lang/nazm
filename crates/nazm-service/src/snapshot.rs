//! Semantic snapshots: one root compilation as a compact manifest of its durable
//! definitions, each with a digest per semantic section Nazm records. `nazm.snapshot/3`
//! (N28; the `effects` section since N36, `provenance` since N38).
//!
//! # The contract
//!
//! **A snapshot says which durable definitions a compilation has, and — for each section the
//! compiler records of them — whether two compilations agree.** It is not a description of
//! what a program does. Two snapshots whose digests are equal agree on the canonical bytes
//! those digests were taken over, and on nothing else: `fn answer() -> Int { 1 }` and
//! `… { 2 }` have the same shape, dependencies, references, callers and callees, and differ
//! only in `source`. Behavioural equivalence, capabilities, tests and history are not
//! recorded, and a snapshot lists them as **unsupported** — never as unchanged. `/1` listed
//! effects there too; N36 records them, which changes what a `/1` reader was told it could
//! never learn, so the snapshot that does is `/2`. N38 adds a `provenance` section, which a
//! `/2` reader would not know to compare — and a provenance summary can move with no byte of
//! the function changing, when a callee's body does — so the snapshot that has it is `/3`.
//!
//! # Which definitions
//!
//! Every function, record and enum the checker declared from a file of the root's
//! compilation — the root and everything it imports, and no other file: nothing is scanned —
//! that has a durable definition key (`module::kind name`). A definition whose module has no
//! durable key (a file reached outside the source root), or whose key the compilation gives
//! to two declarations, is not matched by a path or a spelling instead: it is left out and
//! counted as `untracked`. Built-ins and the core prelude's definitions have no source here
//! and are not definitions of the root.
//!
//! # What each digest is taken over
//!
//! One BLAKE3 digest per section, over a length-prefixed framing of canonical data that
//! names its section, each derived from the layer N27's packet reads it from:
//!
//! | Section | Canonical data |
//! | --- | --- |
//! | `source` | the declaration's exact bytes (N22's range) |
//! | `shape` | kind, whether exported, and the signature (N20) or the fields, or the variants and payloads (N21), in canonical order |
//! | `dependencies` | every entity the declaration uses other than its locals and itself, with a navigable declaration (N17), by identity, with how many uses |
//! | `related_types` | the records and enums it writes by name (N18), by identity, with how many |
//! | `references` | every use of it (N17), by the identity of the definition the use is in, with how many |
//! | `callers`, `callees` | a function's direct checked calls in and out (N25's edge rule), by the identity of the function at the other end, with how many; absent for a record or an enum |
//! | `diagnostics` | code, severity, fix count and range relative to the declaration of every diagnostic whose primary span is inside it — never its message |
//! | `effects` | a function's declared effect set, or that it declares none, and the set its body requires (N36), by name in effect-id order; absent for a record or an enum |
//! | `provenance` | a function's solved provenance summary (N38): the origins its result carries by name in origin-id order, the positions of the parameters it passes on and of those that reach a restricted sink, and the origins each parameter is handed; absent for a record or an enum |
//!
//! Identities, never byte offsets, are what relationship digests are taken over, so a line
//! added above a definition changes no digest of anything below it: the declaration's own
//! place is reported, for navigation, and is not compared. Nothing session-local is hashed
//! or written: no numeric id, generation, host path, time or directory. The same sources
//! give the same bytes from any checkout.
//!
//! # Which program, and which text
//!
//! The compilation of the root exactly as the service analysed it, derived for the request
//! and dropped with it: no snapshot, index or call graph is kept. A compilation that needed
//! syntax recovery anywhere, or could not load a module, has no snapshot — what was lost
//! could be a definition, a use or a call.

use std::collections::BTreeMap;
use std::path::Path;

use nazm_core::sema::ReferenceTarget;
use nazm_span::{FileId, Span};
use serde::Serialize;

use crate::context::{identity_of, incomplete, shape, Identity, Paths, SourceSpan};
use crate::service::{FileKey, LanguageService, Snapshot};
use crate::symbols::{declared_in, SymbolKind};

/// The schema every snapshot declares.
pub const SCHEMA: &str = "nazm.snapshot/3";

/// What a snapshot and a delta cannot know, and say so rather than report as unchanged.
pub const UNSUPPORTED: [&str; 4] = [
    "capabilities",
    "tests",
    "semantic_history",
    "behavioural_equivalence",
];

/// The compiler whose canonical data the digests were taken over. A digest from another
/// build is not comparable, since what a renderer writes may differ.
pub(crate) const COMPILER: &str = concat!("nazm ", env!("CARGO_PKG_VERSION"));

/// A snapshot, or why there is none, exactly as `nazm.snapshot/3` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SnapshotResult {
    Available(Box<SemanticSnapshot>),
    Unavailable(NoSnapshotResult),
}

/// One root compilation's durable definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticSnapshot {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    pub root: RootIdentity,
    /// Ordered by identity; each identity once.
    pub definitions: Vec<SnapshotDefinition>,
    /// Definitions of the compilation with no durable identity, left out.
    pub untracked: usize,
    pub unsupported: [&'static str; 4],
}

/// Which compilation a snapshot is of: its root module's durable key, and the compiler.
/// Nothing that depends on where the checkout sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootIdentity {
    pub module: String,
    pub compiler: String,
}

/// One durable definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotDefinition {
    /// Its durable definition key, `module::kind name`.
    pub identity: String,
    /// `function`, `record` or `enum`.
    pub kind: &'static str,
    /// Where it is declared, for navigation. Not compared.
    pub declaration: SourceSpan,
    pub digests: Digests,
}

/// One lowercase hexadecimal BLAKE3 digest per recorded section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Digests {
    pub source: String,
    pub shape: String,
    pub dependencies: String,
    pub related_types: String,
    pub references: String,
    /// A function's; `None` for a record or an enum, which calls nothing.
    pub callers: Option<String>,
    pub callees: Option<String>,
    pub diagnostics: String,
    /// A function's effects (N36); `None` for a record or an enum.
    pub effects: Option<String>,
    /// A function's provenance summary (N38); `None` for a record or an enum.
    pub provenance: Option<String>,
}

/// Why there is no snapshot, as `nazm.snapshot/3` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoSnapshotResult {
    pub schema: &'static str,
    /// Always `unavailable`.
    pub status: &'static str,
    pub reason: NoSnapshot,
}

/// Why there is no snapshot. There is never a previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoSnapshot {
    /// The root is not open at that version.
    NotCurrent,
    /// Some file of the compilation needed syntax recovery, or a module could not be
    /// loaded.
    IncompleteCompilation,
    /// The root module has no durable key, so nothing can be compared across compilations.
    NoDurableRoot,
}

impl NoSnapshot {
    #[must_use]
    pub const fn result(self) -> SnapshotResult {
        SnapshotResult::Unavailable(NoSnapshotResult {
            schema: SCHEMA,
            status: "unavailable",
            reason: self,
        })
    }
}

impl SnapshotResult {
    /// The one canonical serialisation: compact JSON, fields in declaration order.
    ///
    /// # Panics
    ///
    /// Never: every field is a string, a number, a list or an option of them.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a snapshot is plain data")
    }
}

impl LanguageService {
    /// The semantic snapshot of the compilation rooted at the open document `root`, at
    /// `version`.
    #[must_use]
    pub fn semantic_snapshot(&self, root: &Path, version: i32) -> SnapshotResult {
        match self.derive_snapshot(root, version) {
            Ok(s) => SnapshotResult::Available(Box::new(s)),
            Err(why) => why.result(),
        }
    }

    pub(crate) fn derive_snapshot(
        &self,
        root: &Path,
        version: i32,
    ) -> Result<SemanticSnapshot, NoSnapshot> {
        let s = self.current(root, version).ok_or(NoSnapshot::NotCurrent)?;
        if incomplete(s) {
            return Err(NoSnapshot::IncompleteCompilation);
        }
        let module = s
            .graph
            .module_of(FileId::ROOT)
            .and_then(|m| s.graph.key(m))
            .ok_or(NoSnapshot::NoDurableRoot)?
            .as_str()
            .to_owned();
        let paths = Paths::of(s, root, self.source_root.as_deref());
        let mut m = Manifest::of(s, &paths);
        m.relate(s, &paths);
        let (definitions, untracked) = m.finish(s, &paths);
        Ok(SemanticSnapshot {
            schema: SCHEMA,
            status: "available",
            root: RootIdentity {
                module,
                compiler: COMPILER.to_owned(),
            },
            definitions,
            untracked,
            unsupported: UNSUPPORTED,
        })
    }
}

/// An entity as a relationship digest names it: its kind and durable identity, and — only
/// where it has no durable key — the root-relative file it is declared in, so two such
/// entities are not one. Never a byte offset.
type EntityKey = (u8, Identity, Option<String>);

/// One top-level declaration of the compilation, and what is gathered about it.
struct Decl {
    entity: ReferenceTarget,
    key: EntityKey,
    kind: &'static str,
    exported: Option<bool>,
    file: FileId,
    start: u32,
    end: u32,
    dependencies: BTreeMap<EntityKey, u32>,
    related_types: BTreeMap<EntityKey, u32>,
    references: BTreeMap<EntityKey, u32>,
    callers: BTreeMap<EntityKey, u32>,
    callees: BTreeMap<EntityKey, u32>,
    diagnostics: Vec<(String, &'static str, u32, u32, usize)>,
}

/// The request-local working set: every declaration, and how to find the one a span is in.
struct Manifest {
    decls: Vec<Decl>,
    by_entity: BTreeMap<ReferenceTarget, usize>,
    /// Per file, declarations as `(start, end, index)`, in source order. Top-level
    /// declarations do not overlap.
    by_file: Vec<Vec<(u32, u32, usize)>>,
}

impl Manifest {
    fn of(s: &Snapshot, paths: &Paths) -> Self {
        let mut decls = Vec::new();
        let mut by_entity = BTreeMap::new();
        let mut by_file = vec![Vec::new(); s.files.len()];
        for (i, k) in s.files.iter().enumerate() {
            if !matches!(k, FileKey::Path(_)) {
                continue;
            }
            let file = FileId::from_index(i);
            for sym in declared_in(s, file) {
                let kind = match sym.kind {
                    SymbolKind::Function => "function",
                    SymbolKind::Record => "record",
                    SymbolKind::Enum => "enum",
                    _ => continue,
                };
                let at = decls.len();
                by_entity.insert(sym.target, at);
                if let Some(list) = by_file.get_mut(i) {
                    list.push((sym.start, sym.end, at));
                }
                decls.push(Decl {
                    entity: sym.target,
                    key: entity_key(s, paths, sym.target),
                    kind,
                    exported: sym.exported,
                    file,
                    start: sym.start,
                    end: sym.end,
                    dependencies: BTreeMap::new(),
                    related_types: BTreeMap::new(),
                    references: BTreeMap::new(),
                    callers: BTreeMap::new(),
                    callees: BTreeMap::new(),
                    diagnostics: Vec::new(),
                });
            }
        }
        for list in &mut by_file {
            list.sort_unstable();
        }
        Self {
            decls,
            by_entity,
            by_file,
        }
    }

    /// The declaration `at` is inside, if any.
    fn owner(&self, at: Span) -> Option<usize> {
        let list = self.by_file.get(at.file.index())?;
        let i = list.partition_point(|d| d.0 <= at.start).checked_sub(1)?;
        let &(start, end, decl) = list.get(i)?;
        (start <= at.start && at.end <= end).then_some(decl)
    }

    /// One pass over the reference index, one over the checked calls and one over the
    /// diagnostics, each fact counted against the declaration it is in.
    fn relate(&mut self, s: &Snapshot, paths: &Paths) {
        let res = &s.analysis.resolution;
        for (t, uses) in s.index.iter() {
            let target = self.by_entity.get(&t).copied();
            // A dependency is what N27 would link: not a local, and declared in a file of
            // the compilation.
            let linked = !matches!(t, ReferenceTarget::Local(_))
                && s.index.declaration(t).and_then(|d| paths.span(d)).is_some();
            let typed_kind = matches!(t, ReferenceTarget::Record(_) | ReferenceTarget::Enum(_));
            let key = linked.then(|| entity_key(s, paths, t));
            for u in uses {
                let owner = self.owner(*u);
                if let (Some(d), Some(key)) =
                    (owner.and_then(|o| self.decls.get_mut(o)), key.as_ref())
                {
                    if d.entity != t {
                        *d.dependencies.entry(key.clone()).or_default() += 1;
                        if typed_kind && res.type_name(*u).is_some() {
                            *d.related_types.entry(key.clone()).or_default() += 1;
                        }
                    }
                }
                if let Some(d) = target {
                    if paths.span(*u).is_none() {
                        continue;
                    }
                    let from = match owner.and_then(|o| self.decls.get(o)) {
                        Some(o) => o.key.clone(),
                        None => (u8::MAX, Identity::default(), paths.file(u.file)),
                    };
                    if let Some(d) = self.decls.get_mut(d) {
                        *d.references.entry(from).or_default() += 1;
                    }
                }
            }
        }

        // Direct checked calls between source functions, each call span once.
        let mut edges: Vec<(usize, usize, Span)> = Vec::new();
        for (i, def) in res.functions().iter().enumerate() {
            let from = self
                .by_entity
                .get(&ReferenceTarget::Function(
                    nazm_core::sema::FnId::from_index(i),
                ))
                .copied();
            for site in &def.call_sites {
                let Some(to) = crate::hierarchy::callee(res, site.span) else {
                    continue;
                };
                let to = self.by_entity.get(&ReferenceTarget::Function(to)).copied();
                if let (Some(from), Some(to)) = (from, to) {
                    edges.push((from, to, site.span));
                }
            }
        }
        edges.sort_unstable_by_key(|&(f, t, at)| (f, t, at.file, at.start, at.end));
        edges.dedup();
        for (from, to, _) in edges {
            let (Some(fk), Some(tk)) = (
                self.decls.get(from).map(|d| d.key.clone()),
                self.decls.get(to).map(|d| d.key.clone()),
            ) else {
                continue;
            };
            if let Some(d) = self.decls.get_mut(to) {
                *d.callers.entry(fk).or_default() += 1;
            }
            if let Some(d) = self.decls.get_mut(from) {
                *d.callees.entry(tk).or_default() += 1;
            }
        }

        for d in &s.analysis.diagnostics {
            let p = d.primary.span;
            let Some(decl) = self.owner(p).and_then(|o| self.decls.get_mut(o)) else {
                continue;
            };
            decl.diagnostics.push((
                d.code.to_string(),
                match d.severity {
                    nazm_diag::Severity::Error => "error",
                },
                p.start - decl.start,
                p.end - decl.start,
                d.fixes.len(),
            ));
        }
    }

    /// The durable definitions, ordered by identity, and how many were left out.
    fn finish(self, s: &Snapshot, paths: &Paths) -> (Vec<SnapshotDefinition>, usize) {
        let res = &s.analysis.resolution;
        let mut by_identity: BTreeMap<String, Vec<SnapshotDefinition>> = BTreeMap::new();
        let mut untracked = 0;
        for d in self.decls {
            let Some(identity) = d.key.1.definition.clone() else {
                untracked += 1;
                continue;
            };
            let Some(declaration) = paths.span(Span::new(d.file, d.start, d.end)) else {
                untracked += 1;
                continue;
            };
            let source = s
                .texts
                .get(d.file.index())
                .and_then(|t| t.get(d.start as usize..d.end as usize))
                .unwrap_or_default();
            let function = d.kind == "function";
            let digests = Digests {
                source: Digest::new("source").bytes(source.as_bytes()).hex(),
                shape: shape_digest(res, &d),
                dependencies: counts("dependencies", &d.dependencies),
                related_types: counts("related_types", &d.related_types),
                references: counts("references", &d.references),
                callers: function.then(|| counts("callers", &d.callers)),
                callees: function.then(|| counts("callees", &d.callees)),
                diagnostics: diagnostics_digest(&d.diagnostics),
                effects: function.then(|| effects_digest(res, &d)),
                provenance: function.then(|| provenance_digest(res, &d)),
            };
            by_identity
                .entry(identity.clone())
                .or_default()
                .push(SnapshotDefinition {
                    identity,
                    kind: d.kind,
                    declaration,
                    digests,
                });
        }
        let mut out = Vec::with_capacity(by_identity.len());
        for (_, mut same) in by_identity {
            // One key given to two declarations is not a durable identity for either.
            if same.len() == 1 {
                out.append(&mut same);
            } else {
                untracked += same.len();
            }
        }
        (out, untracked)
    }
}

fn entity_key(s: &Snapshot, paths: &Paths, t: ReferenceTarget) -> EntityKey {
    let rank = match t {
        ReferenceTarget::Function(_) => 0,
        ReferenceTarget::Record(_) => 1,
        ReferenceTarget::Enum(_) => 2,
        ReferenceTarget::Field(_) => 3,
        ReferenceTarget::Variant(_) => 4,
        ReferenceTarget::PayloadField(_) => 5,
        ReferenceTarget::Local(_) => 6,
        ReferenceTarget::Trait(_) => 7,
        ReferenceTarget::TraitMethod(..) => 8,
    };
    let identity = identity_of(s, t);
    let file = if identity.definition.is_none() {
        s.index.declaration(t).and_then(|d| paths.file(d.file))
    } else {
        None
    };
    (rank, identity, file)
}

/// A function's declared set — or that it declares none — and what its body requires.
fn effects_digest(res: &nazm_core::Resolution, d: &Decl) -> String {
    let mut h = Digest::new("effects");
    let f = match d.entity {
        ReferenceTarget::Function(id) => res.function(id),
        _ => None,
    };
    match f.and_then(|f| f.declared_effects) {
        Some(set) => {
            h.field("declared").number(set.names().len());
            for n in set.names() {
                h.field(n);
            }
        }
        None => {
            h.field("undeclared");
        }
    }
    match f.and_then(|f| f.inferred_effects) {
        Some(set) => {
            h.field("required").number(set.names().len());
            for n in set.names() {
                h.field(n);
            }
        }
        None => {
            h.field("unchecked");
        }
    }
    h.hex()
}

/// A function's solved provenance: what its result carries, which parameters it passes on,
/// and which reach a restricted sink.
fn provenance_digest(res: &nazm_core::Resolution, d: &Decl) -> String {
    let mut h = Digest::new("provenance");
    let summary = match d.entity {
        ReferenceTarget::Function(id) => res.function(id).and_then(|f| f.provenance.as_ref()),
        _ => None,
    };
    match summary {
        Some(s) => {
            h.field("origins").number(s.origins.names().len());
            for n in s.origins.names() {
                h.field(n);
            }
            h.field("params").number(s.params.len());
            for p in &s.params {
                h.number(*p as usize);
            }
            h.field("reaches").number(s.reaches.len());
            for p in &s.reaches {
                h.number(*p as usize);
            }
            h.field("receives").number(s.receives.len());
            for r in &s.receives {
                h.number(r.names().len());
                for n in r.names() {
                    h.field(n);
                }
            }
        }
        None => {
            h.field("unsolved");
        }
    }
    h.hex()
}

fn shape_digest(res: &nazm_core::Resolution, d: &Decl) -> String {
    let (signature, fields, variants) = shape(res, d.entity);
    let mut h = Digest::new("shape");
    h.field(d.kind);
    h.field(match d.exported {
        Some(true) => "pub",
        _ => "private",
    });
    if let Some(sig) = signature {
        h.field("signature").field(&sig);
    }
    if let Some(fields) = fields {
        h.field("fields").number(fields.len());
        for f in fields {
            h.field(&f.name).field(&f.ty);
        }
    }
    if let Some(variants) = variants {
        h.field("variants").number(variants.len());
        for v in variants {
            h.field(&v.name).number(v.fields.len());
            for f in v.fields {
                h.field(&f.name).field(&f.ty);
            }
        }
    }
    h.hex()
}

fn counts(section: &str, map: &BTreeMap<EntityKey, u32>) -> String {
    let mut h = Digest::new(section);
    h.number(map.len());
    for ((rank, identity, file), n) in map {
        h.number(usize::from(*rank));
        h.option(identity.definition.as_deref());
        h.option(identity.member.as_deref());
        h.option(file.as_deref());
        h.number(*n as usize);
    }
    h.hex()
}

fn diagnostics_digest(list: &[(String, &'static str, u32, u32, usize)]) -> String {
    let mut sorted: Vec<_> = list.iter().collect();
    sorted.sort_unstable_by(|a, b| (a.2, a.3, &a.0, a.4).cmp(&(b.2, b.3, &b.0, b.4)));
    let mut h = Digest::new("diagnostics");
    h.number(sorted.len());
    for (code, severity, start, end, fixes) in sorted {
        h.field(code)
            .field(severity)
            .number(*start as usize)
            .number(*end as usize)
            .number(*fixes);
    }
    h.hex()
}

/// A digest under construction: the schema and a section name, then length-prefixed fields
/// — `<n>:<bytes>`, so no two sequences of fields share an encoding, and no digest of one
/// section can equal one of another.
struct Digest(blake3::Hasher);

impl Digest {
    fn new(section: &str) -> Self {
        let mut d = Self(blake3::Hasher::new());
        d.field(SCHEMA).field(section);
        d
    }

    fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.0.update(value.len().to_string().as_bytes());
        self.0.update(b":");
        self.0.update(value);
        self
    }

    fn field(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    fn number(&mut self, value: usize) -> &mut Self {
        self.field(&value.to_string())
    }

    /// Present and absent are different fields, and neither is an empty string.
    fn option(&mut self, value: Option<&str>) -> &mut Self {
        match value {
            Some(v) => self.field("+").field(v),
            None => self.field("-"),
        }
    }

    fn hex(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    /// The structural half of "only what the compiler records": this module never looks a
    /// definition up by its spelling, reads a file, scans a directory, asks git, keeps
    /// anything between requests, or stamps a time, a random value or a process id.
    #[test]
    fn a_snapshot_is_never_found_by_spelling_or_from_the_environment() {
        let source = include_str!("snapshot.rs");
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
            ".definition(",
            "by_module",
            "read_to_string",
            "read_dir",
            "fs::",
            "Disk",
            "Command",
            "SystemTime",
            "Instant",
            "process::id",
            "rand",
            "HashMap",
            "HashSet",
            "git",
            "static mut",
            "OnceLock",
            "thread_local",
            ".generation",
            "message",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in snapshot: a digest would not be the compiler's"
            );
        }
    }
}
