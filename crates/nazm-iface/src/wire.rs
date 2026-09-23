//! The persisted shape of a module's interface: `nazm.interface/2`.
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

use std::collections::HashMap;

use nazm_sema::enums::{EnumDef, VariantDef};
use nazm_sema::interface::{EnumExport, Export, RecordExport, UnitInterface, Visibility};
use nazm_sema::key::{DefKey, DefKind, ModuleKey};
use nazm_sema::module::ModuleGraph;
use nazm_sema::record::{FieldDef, RecordDef};
use nazm_sema::resolve::FnId;
use nazm_sema::types::Type;
use nazm_sema::udt::UserTypes;
use nazm_sema::ModuleId;
use nazm_span::Span;

/// The identifier every persisted interface carries.
///
/// `name/major`, the same rule `nazm.diagnostic/1` follows: the major number changes when
/// a reader written against the previous one would be **wrong**.
///
/// # Why N9 versioned it rather than extending `/1`
///
/// `/1` wrote every type as a built-in name. A `/1` reader handed a `/2` file would meet
/// `lib/model.nz::record Point` where it expects `Int`, and — being a reader that
/// validates — would reject it. That is the good case. The bad one is the reason for the
/// bump: a `/1` reader that *did* parse a `/2` file would conclude a module exports a
/// function of a type it cannot represent, and would then answer questions about that
/// module wrongly rather than declining. Compatibility means an old consumer stays
/// **correct**, not that parsing succeeds, and there is no extension of `/1` under which
/// an old reader stays correct about a record it has no concept of.
///
/// # Why N10 versioned it again rather than extending `/2`
///
/// The same argument, one kind over. `/2` had no representation for an enum, so a `/2`
/// reader handed a `/3` file would meet `lib/token.nz::enum Token` where it expects a
/// built-in or a record. Adding an optional `enums` field that an old reader ignores is
/// exactly the bad case: that reader would conclude a module exports a function of a type
/// it cannot represent, and would answer questions about that module **wrongly** rather
/// than declining. Compatibility means an old consumer stays correct, and there is no
/// extension of `/2` under which one stays correct about a variant it has no concept of.
///
/// Unknown majors remain refused rather than reinterpreted — including a future `/4`, and
/// including `/1` and `/2`, which this build no longer reads.
pub const SCHEMA: &str = "nazm.interface/3";

/// One field of a published record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentField {
    /// The field's name, which **is** its identity. Never its position: `docs/spec.md`
    /// makes declaration order non-semantic, so a durable form keyed on an index would
    /// change meaning when an author moved two lines.
    pub name: String,
    /// Its type, in the encoding [`type_name`] produces.
    pub ty: String,
}

/// One variant of a published enum.
///
/// The variant's **name** is its identity, exactly as a field's is. The numeric
/// discriminant is not here and must not be: `docs/spec.md` makes a variant mean its enum
/// and its name, and the number a backend assigns is chosen fresh for each build from the
/// canonical order. A file that wrote the tag would be publishing a representation as a
/// meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentVariant {
    pub name: String,
    /// Its payload fields, sorted by name. Empty for a zero-payload variant.
    pub fields: Vec<PersistentField>,
}

/// One enum definition an importer needs, as published.
///
/// Closed over payload types the same way [`PersistentRecord`] is closed over field types,
/// and for the same reason: an importer has to understand every shape a public enum can
/// hand it, and cannot go and read the module that declared one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentEnum {
    /// Which logical enum this is: `module::enum Name`.
    pub def: String,
    pub name: String,
    /// Its variants, sorted by name.
    ///
    /// Sorted rather than in declaration order, so that moving two variant declarations
    /// produces identical bytes — which is what makes `docs/spec.md`'s claim that variant
    /// order is not semantic true of the fingerprint as well as of the language.
    pub variants: Vec<PersistentVariant>,
}

/// One record definition an importer needs, as published.
///
/// # Why some of these are not exported
///
/// The list is **closed over field types**: if an exported record has a field whose type
/// is a record from a third module, that record's shape is here too, because an importer
/// has to understand it to project through it and cannot go and read the module that
/// declared it. Which of them an importer may *name* is not a flag — it is whether
/// [`Self::def`] belongs to this interface's module, and it cannot be anything else: a
/// private type in a public surface is refused (`N0337`) before an interface is written,
/// so every record of this module that reaches this list is one it exports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentRecord {
    /// Which logical record this is: `module::record Name`.
    pub def: String,
    /// What a dependent writes to name it, when it may. Redundant with the tail of `def`
    /// and kept for the same reason [`PersistentExport::name`] is.
    pub name: String,
    /// Its fields, sorted by name.
    ///
    /// Sorted rather than in declaration order, so that moving two field declarations
    /// produces identical bytes — which is what makes `docs/spec.md`'s claim that field
    /// order is not semantic true of the fingerprint as well as of the language.
    pub fields: Vec<PersistentField>,
}

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
    /// The parameter types, in declaration order, in the encoding [`type_name`] produces.
    pub params: Vec<String>,
    /// The return type, in the same encoding.
    pub ret: String,
}

/// How a type is written in a persisted interface.
///
/// A built-in is its source name. A record is its **durable identity** — `module::record
/// Name` — and not its source name, because the same spelling may name a different record
/// in every module, and an interface that said `Point` would be telling a reader nothing
/// about which `Point`. The two encodings cannot be confused: a built-in's name contains
/// no `::` and a `DefKey`'s always does.
fn type_name(t: Type, types: &UserTypes, graph: &ModuleGraph) -> Option<String> {
    match t {
        Type::Record(id) => {
            let def = types.records().get(id)?;
            // The record's **own** module, which for a closure entry is not the module
            // publishing this interface. The tables hold a session-local `ModuleId`, so
            // the graph is what turns it into something durable.
            let key = graph.key(def.module)?;
            Some(DefKey::new(key.clone(), DefKind::Record, def.name.clone()).to_string())
        }
        Type::Enum(id) => {
            let def = types.enums().get(id)?;
            let key = graph.key(def.module)?;
            Some(DefKey::new(key.clone(), DefKind::Enum, def.name.clone()).to_string())
        }
        other => other.builtin_name().map(ToOwned::to_owned),
    }
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
    /// Every record definition its public surface mentions, sorted by durable identity.
    ///
    /// Before the exports, because an export's signature may refer to one of these and a
    /// reader should meet the definitions first.
    pub records: Vec<PersistentRecord>,
    /// Every enum definition its public surface mentions, sorted by durable identity.
    pub enums: Vec<PersistentEnum>,
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
    /// A user-defined type its public surface mentions is declared in a module with no
    /// durable identity, so there is nothing to write the field's type as. The same answer
    /// as the other two and for the same reason.
    RecordWithoutKey(ModuleId),
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
            Self::RecordWithoutKey(_) => f.write_str(
                "a type in its public surface is declared in a module with no durable \
                 identity, so this interface could not name that type",
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
    /// A type name this build does not have, or a type identity this interface does not
    /// define. A `/3` interface is **closed**: every record and enum a signature, a field
    /// or a payload names is defined in the same file, so a dangling reference is a
    /// malformed interface rather than something to go and look for.
    UnknownType(String),
    /// Two records, two enums, two fields or two variants under one name.
    DuplicateRecord(String),
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
            Self::UnknownType(t) => write!(f, "`{t}` is not a type this interface defines"),
            Self::DuplicateRecord(d) => write!(f, "`{d}` is defined twice"),
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
    pub fn published(
        interface: &UnitInterface,
        graph: &ModuleGraph,
        types: &UserTypes,
    ) -> Result<Self, PublishError> {
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

        // Every record the public surface mentions, each under its own module's key.
        let mut published_records: Vec<PersistentRecord> = Vec::new();
        for r in interface.records() {
            let Some(def) = types.records().get(r.def) else {
                continue;
            };
            let owner = graph
                .key(def.module)
                .ok_or(PublishError::RecordWithoutKey(def.module))?;
            let fields = Self::published_fields(&def.fields, types, graph, def.module)?;
            published_records.push(PersistentRecord {
                def: DefKey::new(owner.clone(), DefKind::Record, def.name.clone()).to_string(),
                name: def.name.clone(),
                fields,
            });
        }
        published_records.sort_by(|a, b| a.def.cmp(&b.def));
        published_records.dedup_by(|a, b| a.def == b.def);

        // Every enum, the same way. Variants sorted by name for the reason fields are:
        // `docs/spec.md` makes variant order non-semantic, and a canonical file is how
        // that becomes true of the fingerprint rather than merely claimed.
        let mut published_enums: Vec<PersistentEnum> = Vec::new();
        for e in interface.enums() {
            let Some(def) = types.enums().get(e.def) else {
                continue;
            };
            let owner = graph
                .key(def.module)
                .ok_or(PublishError::RecordWithoutKey(def.module))?;
            let mut variants: Vec<PersistentVariant> = Vec::new();
            for v in &def.variants {
                variants.push(PersistentVariant {
                    name: v.name.clone(),
                    fields: Self::published_fields(&v.fields, types, graph, def.module)?,
                });
            }
            variants.sort_by(|a, b| a.name.cmp(&b.name));
            published_enums.push(PersistentEnum {
                def: DefKey::new(owner.clone(), DefKind::Enum, def.name.clone()).to_string(),
                name: def.name.clone(),
                variants,
            });
        }
        published_enums.sort_by(|a, b| a.def.cmp(&b.def));
        published_enums.dedup_by(|a, b| a.def == b.def);

        let mut exports: Vec<PersistentExport> = Vec::new();
        for e in interface.exports() {
            let mut params = Vec::with_capacity(e.params.len());
            for t in &e.params {
                params.push(
                    type_name(*t, types, graph)
                        .ok_or_else(|| PublishError::NoModuleKey(interface.module()))?,
                );
            }
            exports.push(PersistentExport {
                def: DefKey::new(module.clone(), DefKind::Function, e.name.clone()).to_string(),
                name: e.name.clone(),
                kind: DefKind::Function.as_str().to_owned(),
                params,
                ret: type_name(e.ret, types, graph)
                    .ok_or_else(|| PublishError::NoModuleKey(interface.module()))?,
            });
        }
        // By name, which is unique within a module. Declaration order is where the author
        // put them and is not part of what the module offers.
        exports.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Self {
            schema: SCHEMA.to_owned(),
            module: module.as_str().to_owned(),
            dependencies,
            records: published_records,
            enums: published_enums,
            exports,
        })
    }

    /// One declared field list, published and sorted by name.
    ///
    /// Shared by a record's fields and a variant's payload because they are the same
    /// thing, exactly as the checker's resolution of them is one function.
    fn published_fields(
        fields: &[FieldDef],
        types: &UserTypes,
        graph: &ModuleGraph,
        owner: nazm_sema::ModuleId,
    ) -> Result<Vec<PersistentField>, PublishError> {
        let mut out: Vec<PersistentField> = Vec::new();
        for f in fields {
            let ty = type_name(f.ty, types, graph).ok_or(PublishError::RecordWithoutKey(owner))?;
            out.push(PersistentField {
                name: f.name.clone(),
                ty,
            });
        }
        // By name: the field list is a set of named fields, and the order the author
        // declared them in is not part of it.
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
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

        // The type definitions first: every type reference in this file must name a
        // built-in or one of these, and a reader that validated signatures before knowing
        // the set would have to guess.
        let mut defined: Vec<&str> = Vec::new();
        let mut previous_record: Option<&str> = None;
        for r in &self.records {
            let key = parse_type_key(&r.def, DefKind::Record)
                .ok_or_else(|| ReadError::BadKey(r.def.clone()))?;
            if key.name() != r.name {
                return Err(ReadError::NameMismatch(r.def.clone()));
            }
            match previous_record {
                Some(p) if p == r.def => return Err(ReadError::DuplicateRecord(r.def.clone())),
                Some(p) if p > r.def.as_str() => {
                    return Err(ReadError::NotCanonical("records".to_owned()))
                }
                _ => {}
            }
            previous_record = Some(&r.def);
            defined.push(&r.def);
            canonical_fields(&r.def, &r.fields)?;
        }
        let mut previous_enum: Option<&str> = None;
        for e in &self.enums {
            let key = parse_type_key(&e.def, DefKind::Enum)
                .ok_or_else(|| ReadError::BadKey(e.def.clone()))?;
            if key.name() != e.name {
                return Err(ReadError::NameMismatch(e.def.clone()));
            }
            match previous_enum {
                Some(p) if p == e.def => return Err(ReadError::DuplicateRecord(e.def.clone())),
                Some(p) if p > e.def.as_str() => {
                    return Err(ReadError::NotCanonical("enums".to_owned()))
                }
                _ => {}
            }
            // A record and an enum cannot share a durable identity, because the kind is
            // part of it — but they can share a *name* across modules, and the list above
            // is what a type reference is resolved against, so both go in it.
            previous_enum = Some(&e.def);
            defined.push(&e.def);
            let mut previous_variant: Option<&str> = None;
            for v in &e.variants {
                match previous_variant {
                    Some(p) if p == v.name => {
                        return Err(ReadError::DuplicateRecord(format!("{}.{}", e.def, v.name)))
                    }
                    Some(p) if p > v.name.as_str() => {
                        return Err(ReadError::NotCanonical("variants".to_owned()))
                    }
                    _ => {}
                }
                previous_variant = Some(&v.name);
                canonical_fields(&format!("{}.{}", e.def, v.name), &v.fields)?;
            }
        }
        // Closed: a field, a payload or a signature may name a built-in or a type this
        // file defines, and nothing else. Checked after the whole set is known, so a
        // forward reference between two definitions is fine and a dangling one is not.
        let referenced = self.records.iter().flat_map(|r| r.fields.iter()).chain(
            self.enums
                .iter()
                .flat_map(|e| e.variants.iter())
                .flat_map(|v| v.fields.iter()),
        );
        for f in referenced {
            if Type::from_name(&f.ty).is_none() && !defined.contains(&f.ty.as_str()) {
                return Err(ReadError::UnknownType(f.ty.clone()));
            }
        }

        let mut previous: Option<&str> = None;
        for e in &self.exports {
            let kind =
                DefKind::parse(&e.kind).ok_or_else(|| ReadError::UnknownKind(e.kind.clone()))?;
            if kind != DefKind::Function {
                // `records` is where a record is published; an export entry claiming to be
                // one would be the same definition in two places.
                return Err(ReadError::UnknownKind(e.kind.clone()));
            }
            for t in e.params.iter().chain(std::iter::once(&e.ret)) {
                if Type::from_name(t).is_none() && !defined.contains(&t.as_str()) {
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
    pub fn rehydrate(&self, module: ModuleId, first: FnId, types: &mut UserTypes) -> UnitInterface {
        // Two passes, for the reason the checker declares type names before resolving
        // field types: a field may name a type defined later in this file, and both
        // orders have to work. One pass over records and enums together, because a record
        // field may name an enum and a payload may name a record.
        let mut ids: HashMap<&str, Type> = HashMap::new();
        let own = self.module_key();
        // A type this interface's module declares is one an importer may name. One from
        // another module is here because a shape mentions it, and naming it would be the
        // transitive visibility `docs/spec.md` refuses — so it is declared **without**
        // registering its name.
        let mine = |def: &str, kind: DefKind| {
            parse_type_key(def, kind)
                .zip(own.as_ref())
                .is_some_and(|(k, m)| k.module() == m)
        };
        for r in &self.records {
            let nameable = mine(&r.def, DefKind::Record);
            let def = RecordDef {
                name: r.name.clone(),
                module,
                visibility: if nameable {
                    Visibility::Exported
                } else {
                    Visibility::Private
                },
                // No span: the exporting file is not open, which is the point. A
                // diagnostic about an imported type points nowhere rather than somewhere
                // wrong.
                name_span: Span::synthetic(),
                fields: Vec::new(),
                layout: Vec::new(),
                // No declaration to go back to. Nothing reads `decl` for a type that came
                // from an interface, because there is no tree it indexes into.
                decl: u32::MAX,
            };
            let id = if nameable {
                types.records_mut().declare(def)
            } else {
                types.records_mut().declare_unnamed(def)
            };
            ids.insert(&r.def, Type::Record(id));
        }
        for e in &self.enums {
            let nameable = mine(&e.def, DefKind::Enum);
            let def = EnumDef {
                name: e.name.clone(),
                module,
                visibility: if nameable {
                    Visibility::Exported
                } else {
                    Visibility::Private
                },
                name_span: Span::synthetic(),
                variants: Vec::new(),
                order: Vec::new(),
                decl: u32::MAX,
            };
            let id = if nameable {
                types.enums_mut().declare(def)
            } else {
                types.enums_mut().declare_unnamed(def)
            };
            ids.insert(&e.def, Type::Enum(id));
        }
        for r in &self.records {
            let Some(Type::Record(id)) = ids.get(r.def.as_str()).copied() else {
                continue;
            };
            let fields = read_fields(&r.fields, &ids);
            types.records_mut().complete(id, fields);
        }
        for e in &self.enums {
            let Some(Type::Enum(id)) = ids.get(e.def.as_str()).copied() else {
                continue;
            };
            let variants = e
                .variants
                .iter()
                .map(|v| VariantDef {
                    name: v.name.clone(),
                    name_span: Span::synthetic(),
                    fields: read_fields(&v.fields, &ids),
                    layout: Vec::new(),
                })
                .collect();
            types.enums_mut().complete(id, variants);
        }

        let exports = self
            .exports
            .iter()
            .enumerate()
            .map(|(i, e)| Export {
                name: e.name.clone(),
                params: e.params.iter().map(|t| read_type(t, &ids)).collect(),
                ret: read_type(&e.ret, &ids),
                name_span: Span::synthetic(),
                def: FnId::from_index(first.index() + i),
            })
            .collect();
        let record_exports = self
            .records
            .iter()
            .filter_map(|r| match ids.get(r.def.as_str()).copied() {
                Some(Type::Record(def)) => Some(RecordExport {
                    def,
                    nameable: mine(&r.def, DefKind::Record),
                }),
                _ => None,
            })
            .collect();
        let enum_exports = self
            .enums
            .iter()
            .filter_map(|e| match ids.get(e.def.as_str()).copied() {
                Some(Type::Enum(def)) => Some(EnumExport {
                    def,
                    nameable: mine(&e.def, DefKind::Enum),
                }),
                _ => None,
            })
            .collect();
        UnitInterface::new(module, exports, record_exports, enum_exports, Vec::new())
    }

    /// This module's durable identity. Validated on read, so this cannot fail afterwards.
    #[must_use]
    pub fn module_key(&self) -> Option<ModuleKey> {
        ModuleKey::parse(&self.module)
    }
}

/// Split `module::record Name` or `module::enum Name` back into a key.
///
/// Written out rather than given to `DefKey` as a parser, because a `DefKey` is a value a
/// compilation computes and this is the one place a *file* claims to be one. Anything that
/// is not in the canonical spelling is refused rather than repaired, and a key whose kind
/// is not the one the caller is reading is refused rather than accepted under the wrong
/// heading — an enum listed among the records is a file that disagrees with itself.
fn parse_type_key(text: &str, want: DefKind) -> Option<DefKey> {
    let (module, rest) = text.split_once("::")?;
    let (kind, name) = rest.split_once(' ')?;
    if DefKind::parse(kind)? != want {
        return None;
    }
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    Some(DefKey::new(ModuleKey::parse(module)?, want, name))
}

/// A published field list must be sorted by name with no duplicate.
///
/// `owner` is only for the message. Checked rather than assumed, because a file is data:
/// bytes that deserialise are still something else's output, and two orderings of one
/// field set would be two files for one interface and two fingerprints for one meaning.
fn canonical_fields(owner: &str, fields: &[PersistentField]) -> Result<(), ReadError> {
    let mut previous: Option<&str> = None;
    for f in fields {
        match previous {
            Some(p) if p == f.name => {
                return Err(ReadError::DuplicateRecord(format!("{owner}.{}", f.name)))
            }
            Some(p) if p > f.name.as_str() => {
                return Err(ReadError::NotCanonical("fields".to_owned()))
            }
            _ => {}
        }
        previous = Some(&f.name);
    }
    Ok(())
}

/// A published field list, as this compilation's fields.
fn read_fields(fields: &[PersistentField], ids: &HashMap<&str, Type>) -> Vec<FieldDef> {
    fields
        .iter()
        .map(|f| FieldDef {
            name: f.name.clone(),
            ty: read_type(&f.ty, ids),
            name_span: Span::synthetic(),
            ty_span: Span::synthetic(),
        })
        .collect()
}

/// A published type string, as a [`Type`] in the reading compilation.
///
/// Validation has already established that every name is either a built-in or one of this
/// file's types, so the fallback is unreachable; it is `Int` rather than a panic for the
/// same reason the checker continues as `Int` after an unknown type.
fn read_type(text: &str, ids: &HashMap<&str, Type>) -> Type {
    Type::from_name(text).unwrap_or_else(|| ids.get(text).copied().unwrap_or(Type::Int))
}
