//! User-defined records: what one is, which fields it has, and what follows from that.
//!
//! # Why a table rather than more variants
//!
//! Before N9 the type universe was six values and every property of a type could be a
//! `match` with six arms. A record's properties are not its own — they are its fields' —
//! so the question *"does this need cleanup"* stopped being answerable by looking at the
//! type and started being answerable by looking at the type **and the definitions in
//! scope**. That is what this table is: the definitions, and the derivations over them.
//!
//! [`Type::Record`](crate::Type) therefore carries a [`RecordId`] and nothing else, and
//! every property a consumer wants is derived from the fields — in
//! [`UserTypes`](crate::UserTypes) since N10, because a record field may now be an enum
//! and the walk crosses both tables. The alternative — copying a field
//! list into the type — would make two records with the same fields the same type, which
//! `docs/spec.md` refuses (records are nominal).
//!
//! # Session-local, like everything else in this crate
//!
//! A [`RecordId`] is an index into one [`Records`], valid for one compilation in one
//! process. The durable identity of a record is a [`DefKey`](crate::DefKey) with
//! [`DefKind::Record`](crate::DefKind), exactly as a function's is, and that is what a
//! persisted interface writes. Nothing may serialise a `RecordId`.

use std::collections::HashMap;

use nazm_span::Span;

use crate::generic::Instance;
use crate::interface::Visibility;
use crate::module::ModuleId;
use crate::types::Type;

/// A record of this compilation, by declaration order.
///
/// Session-local. See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId(u32);

impl RecordId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// One field: a name, a type, and where each was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Type,
    /// Where the field's name is written, for "declared here".
    pub name_span: Span,
    /// Where its type is written, for a diagnostic about the type rather than the field.
    pub ty_span: Span,
}

/// One record definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDef {
    pub name: String,
    /// The module that declares it. Two modules may each declare `Thing`; they are two
    /// types, and `docs/spec.md` says so in the same words it says it for functions.
    pub module: ModuleId,
    pub visibility: Visibility,
    pub name_span: Span,
    /// The fields **in declaration order**, which is what a reader wrote and what a
    /// diagnostic should show.
    ///
    /// Not the layout order, and not part of what the type means: `docs/spec.md` makes
    /// construction named and declaration order non-semantic, so moving two fields is not
    /// a change to this type. [`Self::layout`] is the order the backend uses.
    pub fields: Vec<FieldDef>,
    /// Indices into [`Self::fields`], sorted by field name.
    ///
    /// The **canonical** order: one deterministic sequence derived from the field names,
    /// which are what identifies a field. A backend laying fields out in this order gives
    /// two source orderings of the same record the same layout — so an edit the language
    /// says means nothing also *does* nothing, which is the property `docs/spec.md` would
    /// otherwise be claiming without a mechanism.
    pub layout: Vec<u32>,
    /// This record's index in `Program::records`, so a consumer holding a [`RecordId`]
    /// can reach the declaration it came from. An index into **the program this was
    /// produced from**, exactly as [`FnDef::decl`](crate::resolve::FnDef::decl) is.
    pub decl: u32,
    /// The type parameters' names, in the order declared. Empty unless this record is
    /// generic. Presentation only: a parameter's identity is its **position**, and nothing
    /// that persists or names native code reads these.
    pub params: Vec<String>,
    /// For an **applied** record — `Box[Int]` — the definition it applies and to what.
    ///
    /// An applied record is an entry of its own in this table, interned by that pair, whose
    /// fields are the definition's after substitution. That is what lets every
    /// consumer that reads a definition — copy, drop, layout, task safety, construction —
    /// read an instance by the same code: the substitution happened once, here, and
    /// nothing downstream has to know it did. `None` for a definition as written.
    pub instance: Option<Instance<RecordId>>,
}

impl RecordDef {
    /// The field of this name, with its index in declaration order.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(u32, &FieldDef)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name)
            .map(|(i, f)| (u32::try_from(i).unwrap_or(u32::MAX), f))
    }

    /// The fields in canonical order, each with its declaration index.
    pub fn in_layout_order(&self) -> impl Iterator<Item = (u32, &FieldDef)> {
        self.layout
            .iter()
            .filter_map(|i| self.fields.get(*i as usize).map(|f| (*i, f)))
    }
}

/// Why a record may not cross into a task, and which field is responsible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskUnsafe {
    /// The field path from the outer record to the offending field, by name:
    /// `["inner", "values"]` for `Outer.inner.values`.
    pub path: Vec<String>,
    /// The type that cannot cross — always a sequence today.
    pub ty: Type,
}

/// Every record declared in one compilation, and the properties derived from them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Records {
    defs: Vec<RecordDef>,
    /// Applied definitions, by what they apply: one entry per definition and argument
    /// list, which is what makes `RecordId` equality type equality for an applied type too.
    interned: HashMap<(RecordId, Vec<Type>), RecordId>,
    /// Name to definition, per module — the same shape, and for the same reason, as
    /// [`Resolution`](crate::Resolution)'s function map. There is no global type
    /// namespace.
    by_module: Vec<HashMap<String, RecordId>>,
}

impl Records {
    #[must_use]
    pub fn all(&self) -> &[RecordDef] {
        &self.defs
    }

    #[must_use]
    pub fn get(&self, id: RecordId) -> Option<&RecordDef> {
        self.defs.get(id.index())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The record this module declares under this name — public or private, since a
    /// module sees all of its own.
    #[must_use]
    pub fn declared(&self, module: ModuleId, name: &str) -> Option<RecordId> {
        self.by_module.get(module.index())?.get(name).copied()
    }

    /// Every record declared anywhere under this name, for diagnostics only.
    ///
    /// The same rule as [`Resolution::defined_anywhere`](crate::Resolution::defined_anywhere):
    /// nothing about resolution may consult this, because a resolver that looked at every
    /// module would be the global namespace records deliberately do not have.
    pub fn declared_anywhere<'a>(&'a self, name: &'a str) -> impl Iterator<Item = RecordId> + 'a {
        self.defs
            .iter()
            .enumerate()
            .filter(move |(_, r)| r.name == name)
            .map(|(i, _)| RecordId::from_index(i))
    }

    /// Declare a record with its name and visibility, before its fields are known.
    ///
    /// Two stages because a field's type may name a record declared later, or in another
    /// module — `docs/spec.md` makes declaration order irrelevant, and that is only
    /// possible if every name exists before any field type is resolved. The same shape as
    /// N2's declare-then-check, one level earlier.
    ///
    /// A second declaration of the same name **in the same module** keeps the first, so
    /// one mistake stays one error; the caller reports it.
    pub fn declare(&mut self, def: RecordDef) -> RecordId {
        let id = RecordId::from_index(self.defs.len());
        let m = def.module.index();
        if self.by_module.len() <= m {
            self.by_module.resize_with(m + 1, HashMap::new);
        }
        if let Some(names) = self.by_module.get_mut(m) {
            names.entry(def.name.clone()).or_insert(id);
        }
        self.defs.push(def);
        id
    }

    /// Add a definition **without registering its name**.
    ///
    /// For a record an interface carries because an exported shape mentions it, rather
    /// than because the module exports it: the importer must know the shape and must not
    /// be able to write the name. Registering it would be exactly the transitive
    /// visibility `docs/spec.md` refuses.
    pub fn declare_unnamed(&mut self, def: RecordDef) -> RecordId {
        let id = RecordId::from_index(self.defs.len());
        self.defs.push(def);
        id
    }

    /// The entry for this definition applied to these arguments, if there is one yet.
    #[must_use]
    pub fn instance(&self, template: RecordId, args: &[Type]) -> Option<RecordId> {
        self.interned.get(&(template, args.to_vec())).copied()
    }

    /// Add an applied definition. Never registered by name — `Box[Int]` is not a name a
    /// module declares — and interned, so the caller must have asked [`Self::instance`]
    /// first.
    pub fn declare_instance(&mut self, def: RecordDef) -> RecordId {
        let id = RecordId::from_index(self.defs.len());
        if let Some(inst) = &def.instance {
            self.interned.insert((inst.template, inst.args.clone()), id);
        }
        self.defs.push(def);
        id
    }

    /// Give a declared record its fields, once their types have been resolved.
    ///
    /// `fields` is in declaration order; the canonical layout order is computed here so
    /// that no consumer has to know the rule and none can disagree about it.
    pub fn complete(&mut self, id: RecordId, fields: Vec<FieldDef>) {
        let mut layout: Vec<u32> = (0..u32::try_from(fields.len()).unwrap_or(u32::MAX)).collect();
        layout.sort_by(|a, b| {
            let name = |i: &u32| {
                fields
                    .get(*i as usize)
                    .map(|f| f.name.as_str())
                    .unwrap_or("")
            };
            name(a).cmp(name(b))
        });
        if let Some(def) = self.defs.get_mut(id.index()) {
            def.fields = fields;
            def.layout = layout;
        }
    }
}
