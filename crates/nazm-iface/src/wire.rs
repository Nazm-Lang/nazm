//! The persisted shape of a module's interface: `nazm.interface/1`.
//!
//! # What may appear in it
//!
//! Durable identities, names, and a canonical spelling of each exported signature. That is
//! the whole list, and the omissions are the design:
//!
//! | Not in it | Because |
//! | --- | --- |
//! | `FileId`, `ModuleId`, `FnId` | indices into one compilation; meaningless to the run that reads this |
//! | `Span` | a position in a file, which says nothing about what a module offers — and moving a function must not change its interface |
//! | private definitions | they are not offered |
//! | bodies | the point of an interface is that a dependent needs none |
//! | absolute paths | they say where a checkout lives, and two checkouts of one project must produce identical bytes |
//! | timestamps, build ids, load order | none of them is part of what a module means |
//!
//! # Determinism
//!
//! The same interface must serialise to the same bytes, in this process and in the next
//! one. Every field is a struct field or a `Vec` — **no map**, so no iteration order to
//! depend on — exports are sorted by name and dependencies by key, and `serde_json` emits
//! struct fields in declaration order. `docs/architecture.md` §7.3 says why that matters:
//! a fingerprint over non-deterministic bytes is a fingerprint that changes when nothing
//! did.

use serde::{Deserialize, Serialize};

use nazm_sema::interface::{Export, UnitInterface};
use nazm_sema::key::{DefKey, DefKind, ModuleKey};
use nazm_sema::module::ModuleGraph;
use nazm_sema::resolve::FnId;
use nazm_sema::types::Type;
use nazm_sema::ModuleId;
use nazm_span::Span;

/// The identifier every persisted interface carries.
///
/// `name/major`, the same rule `nazm.diagnostic/1` follows: the major number changes when
/// a reader written against the previous one would be **wrong**. A future user-defined
/// type, a generic parameter or an effect row is a new shape for an exported signature,
/// and this is what stops one being read as the old shape. `docs/diagnostics.md` states
/// the versioning rule in full and this format follows it.
pub const SCHEMA: &str = "nazm.interface/1";

/// One exported definition, as published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentExport {
    /// Which logical definition this is: `module::kind name`.
    pub def: String,
    /// What a dependent writes to name it.
    ///
    /// Redundant with the tail of `def`, and kept because a reader should not have to
    /// parse an identity to learn a name. The two are checked against each other on read.
    pub name: String,
    /// `fn`, today. Spelled as a word so that adding a second kind cannot reinterpret a
    /// value already written.
    pub kind: String,
    /// The parameter types, in declaration order, by their source names.
    pub params: Vec<String>,
    /// The return type, by its source name.
    pub ret: String,
}

/// A module's interface, as persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentUnitInterface {
    /// The shape these bytes are in. First field, so a reader can refuse before parsing
    /// the rest means anything.
    pub schema: String,
    /// Which logical module this is.
    pub module: String,
    /// The modules it directly imports, by durable key, sorted.
    ///
    /// **Direct only**, which is the rule `spec.md` settles. A dependency's own
    /// dependencies are its business and are not part of what this module offers.
    pub dependencies: Vec<String>,
    /// What it offers, sorted by name.
    pub exports: Vec<PersistentExport>,
}

/// Why a persisted interface could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The module has no durable identity — it escapes the source root, or a symbolic
    /// link gave it two names. `docs/spec.md`, *Durable identity*, says why that is an
    /// answer rather than a failure; there is simply nothing to write it under.
    NoModuleKey(ModuleId),
    /// A module it imports has none, and naming a dependency by a session-local id would
    /// be exactly the leak this format exists to prevent.
    DependencyWithoutKey(ModuleId),
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoModuleKey(_) => f.write_str(
                "this module has no durable identity: it is outside the source root, or a \
                 symbolic link gave it more than one name",
            ),
            Self::DependencyWithoutKey(_) => f.write_str(
                "a module it imports has no durable identity, so this interface could not \
                 name its dependencies",
            ),
        }
    }
}

/// Why a persisted interface could not be read.
///
/// Parsing is not trusting. Bytes that deserialise are still data written by something
/// else, and every one of these is a way a file could be well-formed JSON and not an
/// interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// Not JSON, or not this shape.
    Malformed(String),
    /// A schema this build does not implement. **Never** read as the current one: a
    /// future version means a reader written against this one would be wrong, which is
    /// what the version is for.
    UnknownSchema(String),
    /// A module or definition key that is not in the canonical spelling.
    BadKey(String),
    /// A type name this build does not have.
    UnknownType(String),
    /// A definition kind this build does not have.
    UnknownKind(String),
    /// Two exports under one name, which no module can have.
    DuplicateExport(String),
    /// An export whose identity belongs to a different module than the interface's.
    ForeignExport(String),
    /// An export whose `name` and whose `def` disagree.
    NameMismatch(String),
    /// Dependencies or exports not in canonical order, which would mean two files for one
    /// interface and two fingerprints for one meaning.
    NotCanonical(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "not a `{SCHEMA}` interface: {e}"),
            Self::UnknownSchema(s) => write!(
                f,
                "interface schema `{s}` is not `{SCHEMA}`; this build cannot read it"
            ),
            Self::BadKey(k) => write!(f, "`{k}` is not a durable key in canonical form"),
            Self::UnknownType(t) => write!(f, "`{t}` is not a type this build has"),
            Self::UnknownKind(k) => write!(f, "`{k}` is not a definition kind this build has"),
            Self::DuplicateExport(n) => write!(f, "`{n}` is exported twice"),
            Self::ForeignExport(d) => write!(f, "`{d}` is not a definition of this module"),
            Self::NameMismatch(d) => write!(f, "`{d}` disagrees with the name beside it"),
            Self::NotCanonical(w) => write!(f, "{w} are not in canonical order"),
        }
    }
}

impl std::error::Error for ReadError {}

impl PersistentUnitInterface {
    /// Publish an in-memory interface.
    ///
    /// # Errors
    ///
    /// [`PublishError`] when this module or one it imports has no durable identity.
    pub fn published(interface: &UnitInterface, graph: &ModuleGraph) -> Result<Self, PublishError> {
        let module = graph
            .key(interface.module())
            .ok_or(PublishError::NoModuleKey(interface.module()))?;

        let mut dependencies: Vec<String> = Vec::new();
        for d in interface.imports() {
            let key = graph
                .key(d.on)
                .ok_or(PublishError::DependencyWithoutKey(d.on))?;
            dependencies.push(key.as_str().to_owned());
        }
        // Sorted and deduplicated: the *set* of direct dependencies is what a module
        // depends on, and the order the loader happened to walk them in is not part of it.
        dependencies.sort();
        dependencies.dedup();

        let mut exports: Vec<PersistentExport> = interface
            .exports()
            .iter()
            .map(|e| PersistentExport {
                def: DefKey::new(module.clone(), DefKind::Function, e.name.clone()).to_string(),
                name: e.name.clone(),
                kind: DefKind::Function.as_str().to_owned(),
                params: e.params.iter().map(|t| t.name().to_owned()).collect(),
                ret: e.ret.name().to_owned(),
            })
            .collect();
        // By name, which is unique within a module. Declaration order is where the author
        // put them and is not part of what the module offers.
        exports.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self {
            schema: SCHEMA.to_owned(),
            module: module.as_str().to_owned(),
            dependencies,
            exports,
        })
    }

    /// The canonical bytes of this interface.
    ///
    /// # Errors
    ///
    /// Only if serialisation itself fails, which for this shape it does not.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read one back, validating everything a reader would otherwise have to assume.
    ///
    /// # Errors
    ///
    /// [`ReadError`] for anything that is not a `nazm.interface/1` interface this build
    /// can act on. An unknown schema is refused rather than reinterpreted.
    pub fn from_json(text: &str) -> Result<Self, ReadError> {
        // The schema first, and on its own: a future version may not even have the shape
        // this build's `Deserialize` expects, and "failed to parse" would be the wrong
        // thing to tell someone holding a newer file.
        #[derive(Deserialize)]
        struct Envelope {
            schema: String,
        }
        let envelope: Envelope =
            serde_json::from_str(text).map_err(|e| ReadError::Malformed(e.to_string()))?;
        if envelope.schema != SCHEMA {
            return Err(ReadError::UnknownSchema(envelope.schema));
        }

        let parsed: Self =
            serde_json::from_str(text).map_err(|e| ReadError::Malformed(e.to_string()))?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Everything that must be true of an interface beyond its bytes parsing.
    fn validate(&self) -> Result<(), ReadError> {
        let module =
            ModuleKey::parse(&self.module).ok_or_else(|| ReadError::BadKey(self.module.clone()))?;

        let mut deps = Vec::with_capacity(self.dependencies.len());
        for d in &self.dependencies {
            deps.push(ModuleKey::parse(d).ok_or_else(|| ReadError::BadKey(d.clone()))?);
        }
        if deps.windows(2).any(|w| w.first() >= w.last()) {
            return Err(ReadError::NotCanonical("dependencies".to_owned()));
        }

        let mut previous: Option<&str> = None;
        for e in &self.exports {
            let kind =
                DefKind::parse(&e.kind).ok_or_else(|| ReadError::UnknownKind(e.kind.clone()))?;
            for t in e.params.iter().chain(std::iter::once(&e.ret)) {
                if Type::from_name(t).is_none() {
                    return Err(ReadError::UnknownType(t.clone()));
                }
            }
            // The identity must be this module's, and must be the one the name and kind
            // beside it describe. Either mismatch means the file disagrees with itself.
            let expected = DefKey::new(module.clone(), kind, e.name.clone()).to_string();
            if e.def != expected {
                if e.def.starts_with(&format!("{module}::")) {
                    return Err(ReadError::NameMismatch(e.def.clone()));
                }
                return Err(ReadError::ForeignExport(e.def.clone()));
            }
            match previous {
                Some(p) if p == e.name => return Err(ReadError::DuplicateExport(e.name.clone())),
                Some(p) if p > e.name.as_str() => {
                    return Err(ReadError::NotCanonical("exports".to_owned()))
                }
                _ => {}
            }
            previous = Some(&e.name);
        }
        Ok(())
    }

    /// Rebuild an in-memory interface from this one, for **checking a dependent**.
    ///
    /// `module` is the id this interface is bound to in the compilation about to use it,
    /// and `first` is where its definitions are numbered from. Both are session-local and
    /// both are minted by the caller: this file contains no ids and could not, which is
    /// the point of it.
    ///
    /// # What the rebuilt value does and does not carry
    ///
    /// The names, the signatures and the visibility implied by being here — everything
    /// `nazm_core::check_unit` reads. Two things are placeholders and are named as such:
    ///
    /// * **spans** are [`Span::synthetic`](nazm_span::Span::synthetic). A diagnostic about
    ///   an imported definition would normally point into the exporting file, and that
    ///   file is not open — the whole point being that a dependent needs no dependency
    ///   source. So the secondary label points nowhere rather than somewhere wrong;
    /// * the **dependency list is empty**. Checking a dependent never reads a
    ///   dependency's own dependencies — that is the non-transitivity rule, expressed as
    ///   data — so nothing is lost. A future consumer that needs them has the keys in
    ///   [`Self::dependencies`] and must resolve them to ids itself.
    #[must_use]
    pub fn rehydrate(&self, module: ModuleId, first: FnId) -> UnitInterface {
        let exports = self
            .exports
            .iter()
            .enumerate()
            .map(|(i, e)| Export {
                name: e.name.clone(),
                params: e.params.iter().filter_map(|t| Type::from_name(t)).collect(),
                ret: Type::from_name(&e.ret).unwrap_or(Type::Int),
                name_span: Span::synthetic(),
                def: FnId::from_index(first.index() + i),
            })
            .collect();
        UnitInterface::new(module, exports, Vec::new())
    }

    /// This module's durable identity. Validated on read, so this cannot fail afterwards.
    #[must_use]
    pub fn module_key(&self) -> Option<ModuleKey> {
        ModuleKey::parse(&self.module)
    }
}
