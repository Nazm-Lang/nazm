//! Semantic deltas: a baseline `nazm.snapshot/1` against the current compilation of the same
//! root — which durable definitions appeared, disappeared, or changed in a section Nazm
//! records. `nazm.delta/1`. N28.
//!
//! # The contract
//!
//! **A delta compares two snapshots, and says only what their digests say.** Definitions
//! are matched by their durable key and by nothing else: a renamed function is its old key
//! removed and its new key added — a rename is not inferred. A definition in both is
//! `changed` when any recorded section's digest differs, and the change names exactly which
//! sections: `source` alone for a comment, a reformatting or a different literal, with every
//! other section unchanged. That is the whole claim — *the sections Nazm records did not
//! change* — never that the program means the same: effects, capabilities, tests, history
//! and behavioural equivalence are **unsupported**, not unchanged. Where a definition is
//! declared is not a section, so moving one is no change.
//!
//! # A baseline is data
//!
//! It is validated before anything is compared, and refused — never reinterpreted or
//! compared as well as it can be — when it is not `nazm.snapshot/1`, is an unavailable
//! snapshot, has a field this version does not define, names a definition in anything but
//! the canonical key spelling or under the wrong kind, names one twice, carries a digest
//! that is not 64 lowercase hexadecimal digits, or is of another root or another compiler.
//! It is compared with a snapshot derived now, from the compilation as it is: nothing about
//! a previous request is kept, and none is consulted.

use std::collections::BTreeMap;
use std::path::Path;

use nazm_core::sema::{DefKind, ModuleKey};
use serde::{Deserialize, Serialize};

use crate::context::SourceSpan;
use crate::service::LanguageService;
use crate::snapshot::{
    Digests, NoSnapshot, RootIdentity, SemanticSnapshot, SnapshotDefinition, UNSUPPORTED,
};

/// The schema every delta declares.
pub const SCHEMA: &str = "nazm.delta/1";

/// A delta, or why there is none, exactly as `nazm.delta/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DeltaResult {
    Available(Box<SemanticDelta>),
    Unavailable(NoDeltaResult),
}

/// What changed between a baseline snapshot and the current compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticDelta {
    pub schema: &'static str,
    /// Always `available`.
    pub status: &'static str,
    pub root: RootIdentity,
    pub baseline: Counts,
    pub current: Counts,
    /// In the current compilation and not the baseline; where each is declared now.
    pub added: Vec<DeltaDefinition>,
    /// In the baseline and not the current compilation; where each was declared.
    pub removed: Vec<DeltaDefinition>,
    /// In both, with a different digest in at least one recorded section.
    pub changed: Vec<DeltaChange>,
    /// In both, with every recorded section's digest equal.
    pub unchanged: usize,
    pub unsupported: [&'static str; 5],
}

/// How many durable definitions a side has, and how many it left untracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub definitions: usize,
    pub untracked: usize,
}

/// A definition that appeared or disappeared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaDefinition {
    pub identity: String,
    pub kind: String,
    pub declaration: SourceSpan,
}

/// A definition in both whose recorded sections differ, and which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaChange {
    pub identity: String,
    pub kind: String,
    /// Where it is declared now.
    pub declaration: SourceSpan,
    pub sections: Sections,
}

/// For each recorded section, whether its digest differs. `callers` and `callees` are
/// `None` for a record or an enum: not applicable, which is not unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Sections {
    pub source: bool,
    pub shape: bool,
    pub dependencies: bool,
    pub related_types: bool,
    pub references: bool,
    pub callers: Option<bool>,
    pub callees: Option<bool>,
    pub diagnostics: bool,
}

/// Why there is no delta, as `nazm.delta/1` serialises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoDeltaResult {
    pub schema: &'static str,
    /// Always `unavailable`.
    pub status: &'static str,
    pub reason: NoDelta,
}

/// Why there is no delta. There is never a best-effort one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoDelta {
    /// The root is not open at that version.
    NotCurrent,
    /// The current compilation needed syntax recovery, or could not load a module.
    IncompleteCompilation,
    /// The current root module has no durable key.
    NoDurableRoot,
    /// The baseline does not declare `nazm.snapshot/1`.
    BaselineWrongSchema,
    /// The baseline is an unavailable snapshot: there is nothing to compare with.
    BaselineUnavailable,
    /// The baseline is not a well-formed `nazm.snapshot/1`: a missing or unknown field, a
    /// wrong type, a section present for a kind that has none or absent for one that has.
    BaselineMalformed,
    /// A definition's identity is not a canonical durable key of its kind.
    BaselineInvalidIdentity,
    /// Two definitions share one identity.
    BaselineDuplicateIdentity,
    /// A digest is not 64 lowercase hexadecimal digits.
    BaselineInvalidDigest,
    /// The baseline is of another root module.
    BaselineWrongRoot,
    /// The baseline was taken by another compiler, whose digests are not comparable.
    BaselineWrongCompiler,
}

impl NoDelta {
    #[must_use]
    pub const fn result(self) -> DeltaResult {
        DeltaResult::Unavailable(NoDeltaResult {
            schema: SCHEMA,
            status: "unavailable",
            reason: self,
        })
    }
}

impl DeltaResult {
    /// The one canonical serialisation: compact JSON, fields in declaration order.
    ///
    /// # Panics
    ///
    /// Never: every field is a string, a number, a boolean, a list or an option of them.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a delta is plain data")
    }
}

impl LanguageService {
    /// What changed between `baseline`, a `nazm.snapshot/1` object, and the compilation
    /// rooted at the open document `root`, at `version`.
    #[must_use]
    pub fn semantic_delta(
        &self,
        root: &Path,
        version: i32,
        baseline: &serde_json::Value,
    ) -> DeltaResult {
        let baseline = match Baseline::validate(baseline) {
            Ok(b) => b,
            Err(why) => return why.result(),
        };
        let current = match self.derive_snapshot(root, version) {
            Ok(s) => s,
            Err(NoSnapshot::NotCurrent) => return NoDelta::NotCurrent.result(),
            Err(NoSnapshot::IncompleteCompilation) => {
                return NoDelta::IncompleteCompilation.result()
            }
            Err(NoSnapshot::NoDurableRoot) => return NoDelta::NoDurableRoot.result(),
        };
        if baseline.root.module != current.root.module {
            return NoDelta::BaselineWrongRoot.result();
        }
        if baseline.root.compiler != current.root.compiler {
            return NoDelta::BaselineWrongCompiler.result();
        }
        DeltaResult::Available(Box::new(compare(&baseline, current)))
    }
}

fn compare(baseline: &Baseline, current: SemanticSnapshot) -> SemanticDelta {
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = 0;
    let now: BTreeMap<&str, &SnapshotDefinition> = current
        .definitions
        .iter()
        .map(|d| (d.identity.as_str(), d))
        .collect();
    for (identity, d) in &now {
        let Some(before) = baseline.definitions.get(*identity) else {
            added.push(DeltaDefinition {
                identity: d.identity.clone(),
                kind: d.kind.to_owned(),
                declaration: d.declaration.clone(),
            });
            continue;
        };
        let (a, b) = (&before.digests, &d.digests);
        let sections = Sections {
            source: a.source != b.source,
            shape: a.shape != b.shape,
            dependencies: a.dependencies != b.dependencies,
            related_types: a.related_types != b.related_types,
            references: a.references != b.references,
            callers: b.callers.as_ref().map(|c| a.callers.as_ref() != Some(c)),
            callees: b.callees.as_ref().map(|c| a.callees.as_ref() != Some(c)),
            diagnostics: a.diagnostics != b.diagnostics,
        };
        if sections.any() {
            changed.push(DeltaChange {
                identity: d.identity.clone(),
                kind: d.kind.to_owned(),
                declaration: d.declaration.clone(),
                sections,
            });
        } else {
            unchanged += 1;
        }
    }
    let removed = baseline
        .definitions
        .iter()
        .filter(|(identity, _)| !now.contains_key(identity.as_str()))
        .map(|(_, d)| DeltaDefinition {
            identity: d.identity.clone(),
            kind: d.kind.clone(),
            declaration: d.declaration.clone(),
        })
        .collect();
    SemanticDelta {
        schema: SCHEMA,
        status: "available",
        baseline: Counts {
            definitions: baseline.definitions.len(),
            untracked: baseline.untracked,
        },
        current: Counts {
            definitions: current.definitions.len(),
            untracked: current.untracked,
        },
        root: current.root,
        added,
        removed,
        changed,
        unchanged,
        unsupported: UNSUPPORTED,
    }
}

impl Sections {
    fn any(&self) -> bool {
        self.source
            || self.shape
            || self.dependencies
            || self.related_types
            || self.references
            || self.callers == Some(true)
            || self.callees == Some(true)
            || self.diagnostics
    }
}

/// `nazm.snapshot/1` exactly as it is written, and nothing else: an unknown field is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    status: String,
    root: WireRoot,
    definitions: Vec<WireDefinition>,
    untracked: usize,
    unsupported: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRoot {
    module: String,
    compiler: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDefinition {
    identity: String,
    kind: String,
    declaration: WireSpan,
    digests: WireDigests,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSpan {
    file: String,
    start: u32,
    end: u32,
}

/// Every section is required; `callers` and `callees` are required and may be null.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDigests {
    source: String,
    shape: String,
    dependencies: String,
    related_types: String,
    references: String,
    #[serde(deserialize_with = "required_option")]
    callers: Option<String>,
    #[serde(deserialize_with = "required_option")]
    callees: Option<String>,
    diagnostics: String,
}

/// A field that must be present and may be null — serde's default for an `Option` would
/// also accept it missing.
fn required_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

impl WireDefinition {
    fn digests(&self) -> Digests {
        let d = &self.digests;
        Digests {
            source: d.source.clone(),
            shape: d.shape.clone(),
            dependencies: d.dependencies.clone(),
            related_types: d.related_types.clone(),
            references: d.references.clone(),
            callers: d.callers.clone(),
            callees: d.callees.clone(),
            diagnostics: d.diagnostics.clone(),
        }
    }
}

/// What `compare` reads of a baseline definition.
struct Before {
    identity: String,
    kind: String,
    declaration: SourceSpan,
    digests: Digests,
}

impl Baseline {
    fn validate(value: &serde_json::Value) -> Result<Self, NoDelta> {
        if value.get("schema").and_then(serde_json::Value::as_str) != Some(crate::snapshot::SCHEMA)
        {
            return Err(NoDelta::BaselineWrongSchema);
        }
        if value.get("status").and_then(serde_json::Value::as_str) == Some("unavailable") {
            return Err(NoDelta::BaselineUnavailable);
        }
        let wire = Wire::deserialize(value).map_err(|_| NoDelta::BaselineMalformed)?;
        if wire.schema != crate::snapshot::SCHEMA
            || wire.status != "available"
            || wire.unsupported.iter().map(String::as_str).ne(UNSUPPORTED)
            || ModuleKey::parse(&wire.root.module).is_none_or(|m| m.is_prelude())
        {
            return Err(NoDelta::BaselineMalformed);
        }
        let mut definitions = BTreeMap::new();
        for d in wire.definitions {
            let kind = match d.kind.as_str() {
                "function" => DefKind::Function,
                "record" => DefKind::Record,
                "enum" => DefKind::Enum,
                _ => return Err(NoDelta::BaselineMalformed),
            };
            if !canonical_key(&d.identity, kind) {
                return Err(NoDelta::BaselineInvalidIdentity);
            }
            let x = &d.digests;
            let function = kind == DefKind::Function;
            if x.callers.is_some() != function || x.callees.is_some() != function {
                return Err(NoDelta::BaselineMalformed);
            }
            if ![
                Some(&x.source),
                Some(&x.shape),
                Some(&x.dependencies),
                Some(&x.related_types),
                Some(&x.references),
                x.callers.as_ref(),
                x.callees.as_ref(),
                Some(&x.diagnostics),
            ]
            .into_iter()
            .flatten()
            .all(|h| digest(h))
            {
                return Err(NoDelta::BaselineInvalidDigest);
            }
            let before = Before {
                identity: d.identity.clone(),
                kind: d.kind.clone(),
                declaration: SourceSpan {
                    file: d.declaration.file.clone(),
                    start: d.declaration.start,
                    end: d.declaration.end,
                },
                digests: d.digests(),
            };
            if definitions.insert(d.identity, before).is_some() {
                return Err(NoDelta::BaselineDuplicateIdentity);
            }
        }
        Ok(Self {
            root: RootIdentity {
                module: wire.root.module,
                compiler: wire.root.compiler,
            },
            definitions,
            untracked: wire.untracked,
        })
    }
}

/// A validated baseline: its root, and its definitions by identity.
struct Baseline {
    root: RootIdentity,
    definitions: BTreeMap<String, Before>,
    untracked: usize,
}

/// `module::kind name` in exactly the spelling a compilation writes, of `kind`, and not the
/// core prelude's.
fn canonical_key(text: &str, kind: DefKind) -> bool {
    let Some((module, rest)) = text.split_once("::") else {
        return false;
    };
    let Some((word, name)) = rest.split_once(' ') else {
        return false;
    };
    let identifier = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    DefKind::parse(word) == Some(kind)
        && identifier
        && ModuleKey::parse(module).is_some_and(|m| !m.is_prelude())
}

/// Exactly 64 lowercase hexadecimal digits.
fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_accepted_only_in_its_canonical_spelling_and_kind() {
        assert!(canonical_key("main.nz::fn main", DefKind::Function));
        assert!(canonical_key("lib/a.nz::record Point", DefKind::Record));
        assert!(!canonical_key("lib/a.nz::record Point", DefKind::Enum));
        assert!(!canonical_key("./main.nz::fn main", DefKind::Function));
        assert!(!canonical_key("main.nz::fn  main", DefKind::Function));
        assert!(!canonical_key("main.nz::fn main x", DefKind::Function));
        assert!(!canonical_key("main.nz::function main", DefKind::Function));
        assert!(!canonical_key("@core/prelude::enum Option", DefKind::Enum));
        assert!(!canonical_key("main.nz fn main", DefKind::Function));
        assert!(!canonical_key("main.nz::fn ", DefKind::Function));
    }

    #[test]
    fn a_digest_is_exactly_sixty_four_lowercase_hex_digits() {
        let d = "0".repeat(64);
        assert!(digest(&d));
        assert!(!digest(&"0".repeat(63)));
        assert!(!digest(&"A".repeat(64)));
        assert!(!digest(&format!("{}g", "0".repeat(63))));
    }
}
