//! What checking a module decided about its names, in a form another process can read:
//! `nazm.resolved/1`. N77, `docs/internal/architecture.md` §7.78.
//!
//! # What it is
//!
//! The checker's answers — never a second computation of them — translated from session
//! ids into durable identities: which modules a module imports and through which spelling,
//! which definitions it declares, and what every resolved occurrence in its text refers to.
//! A function, record or enum is its [`DefKey`]; a variant, field or payload field is its
//! owner's key and its member's name.
//!
//! # What is deliberately not in it
//!
//! Locals, whose identity is a slot in one body; session ids of any kind; absolute paths,
//! timestamps and load order; types and slots, which is why a build cannot lower from it.
//! Ranges are byte offsets into the source the unit's fingerprint names, so they mean
//! something exactly as long as that fingerprint does.
//!
//! # Determinism
//!
//! Imports keep source order — that order is part of the source, and so of the key the unit
//! is stored under. Definitions and occurrences are sorted by range and then by target, so
//! the bytes depend on nothing a process chose.

use serde::{Deserialize, Serialize};

use nazm_sema::module::{ModuleGraph, ModuleId};
use nazm_sema::references::{ReferenceIndex, ReferenceTarget};
use nazm_sema::resolve::Resolution;

/// The identifier every resolved unit carries. `name/major`, as every machine schema here.
pub const SCHEMA: &str = "nazm.resolved/1";

/// One `use`, as it was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedImport {
    /// The literal as written, without its quotes.
    pub written: String,
    /// The name it was imported under, if any.
    pub alias: Option<String>,
    /// The durable key of the module it names.
    pub module: String,
    /// The fingerprint of that module's interface when this one was checked against it, or
    /// `None` when the compilation gave it none (a module with no durable identity).
    pub interface: Option<String>,
}

/// What a name refers to, durably.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DurableTarget {
    /// The defining item: `module::kind name`.
    pub item: String,
    /// The member of that item, for a variant, a field or a payload field
    /// (`Variant.field`); `None` for the item itself.
    pub member: Option<String>,
    /// `function`, `record`, `enum`, `variant`, `field` or `payload field`.
    pub kind: String,
}

/// One definition the module declares.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolvedDefinition {
    /// Where its name is written.
    pub start: u32,
    pub end: u32,
    pub target: DurableTarget,
}

/// One occurrence of a declared entity's name in the module's text, other than its
/// declaration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolvedOccurrence {
    pub start: u32,
    pub end: u32,
    pub target: DurableTarget,
}

/// One module's resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUnit {
    pub schema: String,
    /// The module's durable key.
    pub module: String,
    /// The fingerprint of the source the ranges are offsets into.
    pub source: String,
    pub imports: Vec<ResolvedImport>,
    pub definitions: Vec<ResolvedDefinition>,
    pub occurrences: Vec<ResolvedOccurrence>,
}

/// Why a unit could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unwritable {
    /// The module has no durable key, so nothing it decides can be named across processes.
    NoKey,
    /// A target lives in a module with no durable key.
    TargetWithoutKey,
}

impl ResolvedUnit {
    /// The unit of `module`, from the checker's resolution of the compilation it is in.
    ///
    /// `source` is the module's source fingerprint and `interface` each module's interface
    /// fingerprint (by module index), both as the caller computed them for the check key;
    /// `written` gives the text of the import literal at a span. `index` is
    /// [`ReferenceIndex::of`] the same resolution, computed once by the caller for every
    /// module.
    ///
    /// # Errors
    ///
    /// [`Unwritable`] when the module, or a module one of its targets is declared in, has
    /// no durable key: such a unit would have to name something with a session id.
    pub fn of(
        res: &Resolution,
        index: &ReferenceIndex,
        graph: &ModuleGraph,
        module: ModuleId,
        source: &str,
        interface: &dyn Fn(ModuleId) -> Option<String>,
        written: &dyn Fn(nazm_span::Span) -> String,
    ) -> Result<Self, Unwritable> {
        let key = graph.key(module).ok_or(Unwritable::NoKey)?;
        let file = graph.file(module).ok_or(Unwritable::NoKey)?;
        let imports = graph
            .imports(module)
            .iter()
            .filter(|d| d.at != nazm_span::Span::synthetic() && d.at.file == file)
            .map(|d| {
                Ok(ResolvedImport {
                    written: written(d.at),
                    alias: d.aliases.first().map(|(a, _)| a.clone()),
                    module: graph
                        .key(d.on)
                        .ok_or(Unwritable::TargetWithoutKey)?
                        .as_str()
                        .to_owned(),
                    interface: interface(d.on),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut definitions = Vec::new();
        let mut occurrences = Vec::new();
        for (target, uses) in index.iter() {
            // Only what this module's text mentions: a target elsewhere — in a module with
            // no durable key, say — is not this unit's to name.
            let declared_here = index.declaration(target).filter(|d| d.file == file);
            if declared_here.is_none() && !uses.iter().any(|u| u.file == file) {
                continue;
            }
            let Some(durable) = durable(res, graph, target)? else {
                continue;
            };
            if let Some(d) = declared_here {
                definitions.push(ResolvedDefinition {
                    start: d.start,
                    end: d.end,
                    target: durable.clone(),
                });
            }
            for u in uses.iter().filter(|u| u.file == file) {
                occurrences.push(ResolvedOccurrence {
                    start: u.start,
                    end: u.end,
                    target: durable.clone(),
                });
            }
        }
        definitions.sort();
        occurrences.sort();
        occurrences.dedup();
        Ok(Self {
            schema: SCHEMA.to_owned(),
            module: key.as_str().to_owned(),
            source: source.to_owned(),
            imports,
            definitions,
            occurrences,
        })
    }

    /// The unit as one JSON document: the stored form and the printed one.
    ///
    /// # Errors
    ///
    /// Never for a unit built by [`ResolvedUnit::of`]; propagated from `serde_json`.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read a unit back, refusing any other schema rather than reinterpreting it.
    ///
    /// # Errors
    ///
    /// When the bytes are not a `nazm.resolved/1` unit.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let unit: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if unit.schema != SCHEMA {
            return Err(format!("`{}` is not `{SCHEMA}`", unit.schema));
        }
        Ok(unit)
    }
}

/// A reference target as a durable identity, or `None` for a local, which has none.
fn durable(
    res: &Resolution,
    graph: &ModuleGraph,
    target: ReferenceTarget,
) -> Result<Option<DurableTarget>, Unwritable> {
    let types = res.types();
    let key_of = |m: ModuleId| graph.key(m).ok_or(Unwritable::TargetWithoutKey);
    let item = |owner: Option<String>| owner.ok_or(Unwritable::TargetWithoutKey);
    let trait_key = |t: nazm_sema::traits::TraitId| -> Result<(String, &nazm_sema::traits::TraitDef), Unwritable> {
        let d = res.traits().get(t).ok_or(Unwritable::TargetWithoutKey)?;
        let module = key_of(d.module)?;
        Ok((
            nazm_sema::DefKey::new(module.clone(), nazm_sema::DefKind::Trait, d.name.clone())
                .to_string(),
            d,
        ))
    };
    Ok(Some(match target {
        ReferenceTarget::Local(_) => return Ok(None),
        // A constant is its own module's (Gate 2): nothing outside it names one, so it has no
        // durable identity to publish, as a local has none.
        ReferenceTarget::Const(_) => return Ok(None),
        // N78: a trait, and a trait's method as its trait's key and its name.
        ReferenceTarget::Trait(t) => DurableTarget {
            item: trait_key(t)?.0,
            member: None,
            kind: "trait".to_owned(),
        },
        ReferenceTarget::TraitMethod(t, m) => {
            let (item, d) = trait_key(t)?;
            DurableTarget {
                item,
                member: Some(
                    d.methods
                        .get(m as usize)
                        .ok_or(Unwritable::TargetWithoutKey)?
                        .name
                        .clone(),
                ),
                kind: "trait method".to_owned(),
            }
        }
        ReferenceTarget::Function(id) => {
            let f = res.function(id).ok_or(Unwritable::TargetWithoutKey)?;
            let k = res.def_key(id, key_of(f.module)?).map(|k| k.to_string());
            DurableTarget {
                item: item(k)?,
                member: None,
                kind: "function".to_owned(),
            }
        }
        ReferenceTarget::Record(id) => {
            let r = types
                .records()
                .get(id)
                .ok_or(Unwritable::TargetWithoutKey)?;
            DurableTarget {
                item: item(res.record_key(id, key_of(r.module)?).map(|k| k.to_string()))?,
                member: None,
                kind: "record".to_owned(),
            }
        }
        ReferenceTarget::Enum(id) => {
            let e = types.enums().get(id).ok_or(Unwritable::TargetWithoutKey)?;
            DurableTarget {
                item: item(res.enum_key(id, key_of(e.module)?).map(|k| k.to_string()))?,
                member: None,
                kind: "enum".to_owned(),
            }
        }
        ReferenceTarget::Field(f) => {
            let r = types
                .records()
                .get(f.record)
                .ok_or(Unwritable::TargetWithoutKey)?;
            let name = r
                .fields
                .get(f.field as usize)
                .ok_or(Unwritable::TargetWithoutKey)?;
            DurableTarget {
                item: item(
                    res.record_key(f.record, key_of(r.module)?)
                        .map(|k| k.to_string()),
                )?,
                member: Some(name.name.clone()),
                kind: "field".to_owned(),
            }
        }
        ReferenceTarget::Variant(v) => {
            let e = types
                .enums()
                .get(v.enumeration)
                .ok_or(Unwritable::TargetWithoutKey)?;
            let variant = e
                .variants
                .get(v.variant as usize)
                .ok_or(Unwritable::TargetWithoutKey)?;
            DurableTarget {
                item: item(
                    res.enum_key(v.enumeration, key_of(e.module)?)
                        .map(|k| k.to_string()),
                )?,
                member: Some(variant.name.clone()),
                kind: "variant".to_owned(),
            }
        }
        ReferenceTarget::PayloadField(p) => {
            let e = types
                .enums()
                .get(p.enumeration)
                .ok_or(Unwritable::TargetWithoutKey)?;
            let variant = e
                .variants
                .get(p.variant as usize)
                .ok_or(Unwritable::TargetWithoutKey)?;
            let field = variant
                .fields
                .get(p.field as usize)
                .ok_or(Unwritable::TargetWithoutKey)?;
            DurableTarget {
                item: item(
                    res.enum_key(p.enumeration, key_of(e.module)?)
                        .map(|k| k.to_string()),
                )?,
                member: Some(format!("{}.{}", variant.name, field.name)),
                kind: "payload field".to_owned(),
            }
        }
    }))
}
