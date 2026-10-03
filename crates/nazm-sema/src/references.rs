//! Where each entity a program declares is used: resolution, read backwards. N17.
//!
//! # What a reference is
//!
//! A **reference** is a source occurrence that this compilation's resolution resolved to a
//! specific entity. Not an identifier with a matching spelling, not a mention in a comment
//! or a string, not a name the checker could not resolve, and not a token the concrete tree
//! merely knows is a name: exactly the spans [`Resolution`] recorded an answer for, and
//! nothing else.
//!
//! # What this is
//!
//! [`Resolution`] answers *this occurrence → which entity*. This is the same answer turned
//! around — *this entity → which occurrences* — computed once, from the resolution alone:
//!
//! - the entities, and where each is declared, from the definition tables the checker built
//!   (functions and their locals, records and their fields, enums, their variants and their
//!   payload fields);
//! - the uses, from the answers the checker recorded, span by span.
//!
//! **It resolves nothing.** It never reads a name to decide what anything means, holds no
//! scope, and cannot be wrong about a use in a way the checker was not: a span is in an
//! entity's set because the checker put that entity against that span. The declaration is
//! kept apart from the uses — a definition is not a use of itself — and a consumer that
//! wants both, as an editor's "include the declaration" does, combines them itself.
//!
//! # Identity
//!
//! A [`ReferenceTarget`] is the identity resolution already has, and nothing stronger:
//! [`FnId`], [`RecordId`] and [`EnumId`] with a declaration index for what they own, and a
//! [`LocalRef`] — a slot *of a function* — for a binding. All of it is **session-local**:
//! valid for this compilation and meaningless in any other. Nothing here is durable, and
//! nothing here may be persisted; a module's interface describes what an importer needs,
//! not where anybody uses it.
//!
//! One adjustment is made, and it is not a resolution: an **applied** record or enum —
//! `Box[Int]` — is its own entry in the type tables, with fields copied from the
//! definition's, so a use through `Box[Int]` and a use through `Box[Str]` carry two ids for
//! one written definition. Each is taken to the definition it applies, by the table's own
//! record of that ([`crate::generic::Instance`]), so a field has one set of uses however
//! many instances reach it.
//!
//! # What is not here
//!
//! A built-in — a call to one, or a built-in type — refers to nothing declared; a type
//! parameter's name is not recorded. A binding introduced but never mentioned has a
//! declaration and no uses. Until N18 a type name written in an annotation or a generic
//! argument, and the enum before the `.` of `E.V(…)`, had no entry either; the checker now
//! records the record or enum each names, and they are uses here like any other.
//!
//! # Completeness, stated per kind
//!
//! [`ReferenceTarget::rename_coverage`] says, for each kind of entity, whether every
//! occurrence whose spelling is the entity's name is recorded — which is what a rename
//! needs, and more than a references query does. It is an exhaustive `match`, so a new kind
//! of entity cannot be added without saying; a new *position* in which an existing kind can
//! be written is held by the corpus gate in `crates/nazm-service/tests/references.rs`, which
//! requires every name token of the repository's source to be indexed or explained.

use std::collections::HashMap;

use nazm_span::Span;

use crate::enums::EnumId;
use crate::record::RecordId;
use crate::resolve::{FieldRef, FnId, LocalRef, PayloadRef, Resolution, VariantRef};

/// An entity a program declares, which an occurrence can refer to.
///
/// Its identity is resolution's: two entities with one spelling are two targets, and one
/// entity reached by two spellings — there are none yet — would be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceTarget {
    Function(FnId),
    /// A parameter, a `let` or a pattern binding.
    Local(LocalRef),
    Record(RecordId),
    /// An enum, by the name it is declared and written with — distinct from any of its
    /// variants. N18.
    Enum(EnumId),
    Variant(VariantRef),
    Field(FieldRef),
    PayloadField(PayloadRef),
    /// A trait, by its declaration (N78).
    Trait(crate::traits::TraitId),
    /// A trait's method, by its trait and position: its declaration in the trait, and every
    /// call through a bound. A call on a concrete type is a use of its impl's function.
    TraitMethod(crate::traits::TraitId, u32),
}

impl ReferenceTarget {
    /// What kind of entity this is, in the words an editor shows.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Function(_) => "function",
            Self::Local(_) => "local",
            Self::Record(_) => "record",
            Self::Enum(_) => "enum",
            Self::Variant(_) => "variant",
            Self::Field(_) => "field",
            Self::PayloadField(_) => "payload field",
            Self::Trait(_) => "trait",
            Self::TraitMethod(..) => "trait method",
        }
    }
}

/// Whether the index holds every occurrence of an entity's name — the precondition of
/// renaming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Every position the language lets this kind of entity be written in is recorded by
    /// the checker, as the entity it names.
    Complete,
    /// Some position is not, and a rename would miss it.
    Incomplete(&'static str),
}

impl ReferenceTarget {
    /// Whether every occurrence of this kind of entity's name is recorded, by the kind: the
    /// positions the grammar gives it, each of which the checker resolves and records.
    ///
    /// No arm is a wildcard. A new kind of entity is a compile error here until someone
    /// says what its occurrences are.
    #[must_use]
    pub const fn rename_coverage(self) -> Coverage {
        match self {
            // Its declaration and its calls, `spawn` included. There are no function
            // values and no import lists: a function's name appears nowhere else.
            Self::Function(_) => Coverage::Complete,
            // A parameter, a `let` or a pattern binding, and every mention and assignment
            // of it — all in one body.
            Self::Local(_) => Coverage::Complete,
            // Its declaration, its constructions, and its name in every written type:
            // parameter, return, field, payload field, type argument at any depth.
            Self::Record(_) => Coverage::Complete,
            // Its declaration, its name in every written type, and the qualifier of every
            // variant construction and pattern.
            Self::Enum(_) => Coverage::Complete,
            // Its declaration, and the name after the `.` of every construction and pattern.
            Self::Variant(_) => Coverage::Complete,
            // Its declaration, every projection and assignment path, and every label of a
            // construction.
            Self::Field(_) => Coverage::Complete,
            // Its declaration, and the label of every construction and pattern field.
            Self::PayloadField(_) => Coverage::Complete,
            // Named in bounds, impl headers and qualified calls, all recorded — but each impl
            // also writes the trait's methods by name, and rename does not edit those yet.
            Self::Trait(_) | Self::TraitMethod(..) => {
                Coverage::Incomplete("a trait's methods are written again in every impl of it")
            }
        }
    }
}

/// One entity's declaration and uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Entry {
    declaration: Option<Span>,
    /// Sorted by file, then start, then end; no span twice.
    uses: Vec<Span>,
}

/// Every entity of one compilation with where it is declared and used, and every indexed
/// occurrence with its entity. Derived from a [`Resolution`] and valid exactly as long as it
/// is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceIndex {
    /// Every occurrence — declaration or use — to the entity it is of.
    at: HashMap<Span, ReferenceTarget>,
    entries: HashMap<ReferenceTarget, Entry>,
}

impl ReferenceIndex {
    /// The index of everything `res` resolved.
    #[must_use]
    pub fn of(res: &Resolution) -> Self {
        let mut index = Self::default();
        for (d, f) in res.functions().iter().enumerate() {
            let function = FnId::from_index(d);
            // A closure has no name to declare (N50): its `fn` is a keyword, not a definition
            // a reader could go to. Its parameters and locals are still declarations.
            if f.closure.is_none() {
                index.declare(ReferenceTarget::Function(function), f.name_span);
            }
            for (slot, l) in f.locals.iter().enumerate() {
                let slot = u32::try_from(slot).unwrap_or(u32::MAX);
                index.declare(ReferenceTarget::Local(LocalRef { function, slot }), l.span);
            }
        }
        for (r, def) in res.records().all().iter().enumerate() {
            if def.instance.is_some() {
                continue;
            }
            let record = RecordId::from_index(r);
            index.declare(ReferenceTarget::Record(record), def.name_span);
            for (field, f) in def.fields.iter().enumerate() {
                let field = u32::try_from(field).unwrap_or(u32::MAX);
                index.declare(
                    ReferenceTarget::Field(FieldRef { record, field }),
                    f.name_span,
                );
            }
        }
        for (e, def) in res.enums().all().iter().enumerate() {
            if def.instance.is_some() {
                continue;
            }
            let enumeration = EnumId::from_index(e);
            index.declare(ReferenceTarget::Enum(enumeration), def.name_span);
            for (variant, v) in def.variants.iter().enumerate() {
                let variant = u32::try_from(variant).unwrap_or(u32::MAX);
                index.declare(
                    ReferenceTarget::Variant(VariantRef {
                        enumeration,
                        variant,
                    }),
                    v.name_span,
                );
                for (field, f) in v.fields.iter().enumerate() {
                    let field = u32::try_from(field).unwrap_or(u32::MAX);
                    index.declare(
                        ReferenceTarget::PayloadField(PayloadRef {
                            enumeration,
                            variant,
                            field,
                        }),
                        f.name_span,
                    );
                }
            }
        }
        // Traits (N78): each declaration and its methods'; every mention; every call
        // through a bound.
        for (t, def) in res.traits().iter() {
            index.declare(ReferenceTarget::Trait(t), def.name_span);
            for (m, method) in def.methods.iter().enumerate() {
                let m = u32::try_from(m).unwrap_or(u32::MAX);
                index.declare(ReferenceTarget::TraitMethod(t, m), method.name_span);
            }
        }
        for (at, t) in res.traits().mentions() {
            index.used(at, ReferenceTarget::Trait(t));
        }
        for (at, c) in res.traits().trait_calls() {
            index.used(at, ReferenceTarget::TraitMethod(c.trait_id, c.method));
        }
        res.each_use(|span, target| index.used(span, definition_of(res, target)));
        for entry in index.entries.values_mut() {
            entry.uses.sort_unstable();
            entry.uses.dedup();
        }
        index
    }

    fn declare(&mut self, target: ReferenceTarget, at: Span) {
        if at == Span::synthetic() {
            return;
        }
        self.at.entry(at).or_insert(target);
        self.entries.entry(target).or_default().declaration = Some(at);
    }

    /// A use of `target` at `at` — unless `at` is where `target` is declared, which the
    /// checker records for a `let` so the backend can find its slot, and which is not a
    /// use.
    fn used(&mut self, at: Span, target: ReferenceTarget) {
        if at == Span::synthetic() {
            return;
        }
        // One occurrence has one meaning. The kinds are recorded in disjoint positions, so
        // two answers for one span would be a checker bug; the first kind wins, in
        // `each_use`'s fixed order, rather than whichever a map yields first.
        if self.at.get(&at).is_some_and(|t| *t != target) {
            return;
        }
        self.at.insert(at, target);
        let entry = self.entries.entry(target).or_default();
        if entry.declaration != Some(at) {
            entry.uses.push(at);
        }
    }

    /// The entity the occurrence at exactly `at` is of: a use or a declaration. `None` for
    /// any span resolution recorded nothing for.
    #[must_use]
    pub fn target(&self, at: Span) -> Option<ReferenceTarget> {
        self.at.get(&at).copied()
    }

    /// Where `target` is declared, if it is declared in source.
    #[must_use]
    pub fn declaration(&self, target: ReferenceTarget) -> Option<Span> {
        self.entries.get(&target)?.declaration
    }

    /// Every use of `target`, ordered by file, start and end, each once. Never the
    /// declaration.
    #[must_use]
    pub fn uses(&self, target: ReferenceTarget) -> &[Span] {
        self.entries.get(&target).map_or(&[], |e| e.uses.as_slice())
    }

    /// How many entities are indexed.
    #[must_use]
    pub fn targets(&self) -> usize {
        self.entries.len()
    }

    /// How many occurrences are indexed — declarations and uses — each once.
    #[must_use]
    pub fn occurrences(&self) -> usize {
        self.at.len()
    }

    /// How many uses are indexed, over every entity.
    #[must_use]
    pub fn use_count(&self) -> usize {
        self.entries.values().map(|e| e.uses.len()).sum()
    }

    /// Every entity with its uses, in target order. For audits and tests: what an editor
    /// asks is [`Self::target`] and then [`Self::uses`].
    pub fn iter(&self) -> impl Iterator<Item = (ReferenceTarget, &[Span])> {
        let mut all: Vec<(ReferenceTarget, &[Span])> = self
            .entries
            .iter()
            .map(|(t, e)| (*t, e.uses.as_slice()))
            .collect();
        all.sort_unstable_by_key(|(t, _)| *t);
        all.into_iter()
    }
}

/// The written definition an entity reached through an applied type belongs to.
///
/// `Box[Int].value` and `Box[Str].value` are one field of one definition; the type tables
/// hold them under two ids because each instance has its own substituted fields. The table
/// records which definition each instance applies, and this reads it. A definition is its
/// own answer.
fn definition_of(res: &Resolution, target: ReferenceTarget) -> ReferenceTarget {
    let record = |id: RecordId| {
        res.records()
            .get(id)
            .and_then(|d| d.instance.as_ref())
            .map_or(id, |i| i.template)
    };
    let enumeration = |id: EnumId| {
        res.enums()
            .get(id)
            .and_then(|d| d.instance.as_ref())
            .map_or(id, |i| i.template)
    };
    match target {
        ReferenceTarget::Record(id) => ReferenceTarget::Record(record(id)),
        ReferenceTarget::Enum(id) => ReferenceTarget::Enum(enumeration(id)),
        ReferenceTarget::Field(f) => ReferenceTarget::Field(FieldRef {
            record: record(f.record),
            ..f
        }),
        ReferenceTarget::Variant(v) => ReferenceTarget::Variant(VariantRef {
            enumeration: enumeration(v.enumeration),
            ..v
        }),
        ReferenceTarget::PayloadField(p) => ReferenceTarget::PayloadField(PayloadRef {
            enumeration: enumeration(p.enumeration),
            ..p
        }),
        // A closure's copy of a captured binding is that binding (N50).
        ReferenceTarget::Local(l) => ReferenceTarget::Local(res.binding_of(l)),
        other @ (ReferenceTarget::Function(_)
        | ReferenceTarget::Trait(_)
        | ReferenceTarget::TraitMethod(..)) => other,
    }
}

#[cfg(test)]
mod tests {
    /// The structural half of "it resolves nothing": the index never looks a name up. It
    /// reads the definition tables by position and the recorded uses by span, and every
    /// spelling-keyed search the tables offer — which exist for the checker and its
    /// diagnostics — is out of reach of this file.
    #[test]
    fn the_index_never_looks_a_name_up() {
        let source = include_str!("references.rs");
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
            // Spelled in two halves so the `one resolver` gate does not read this list as
            // a call.
            concat!("defined", "_anywhere"),
            ".declared(",
            ".field(&",
            ".variant(&",
            ".name ==",
            "== name",
            "HashMap<String",
            "BTreeMap<String",
        ] {
            assert!(
                !code.contains(forbidden),
                "`{forbidden}` in the reference index: it would be resolving by spelling"
            );
        }
    }
}
