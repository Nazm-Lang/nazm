//! User-defined enums: a closed set of named variants, one of which is active.
//!
//! # What this table is, and what it is not
//!
//! The same shape as [`Records`](crate::record::Records) and for the same reason: an
//! enum's properties are not its own, they are its variants' payload fields'. What differs
//! is *which* composition applies, and that difference is the whole of N10:
//!
//! | | record | enum |
//! | --- | --- | --- |
//! | copy | every field | the **active** variant's fields |
//! | destroy | every field | the **active** variant's fields |
//! | needs cleanup | some field does | some field of **some** variant does |
//! | task safety | every field may cross | every field of **every** variant may cross |
//!
//! The first two rows are runtime questions about a value and the last two are static
//! questions about a type, which is why `docs/spec.md` gives task safety no
//! flow-sensitive escape: a `spawn` is handed a value of a type, and the type is what the
//! guarantee is about.
//!
//! The derivations themselves are **not here**. They cross kinds — a record may contain an
//! enum and an enum may contain a record — so they live in [`UserTypes`](crate::UserTypes),
//! which can see both tables. This module is the definitions.
//!
//! # Session-local, like everything else in this crate
//!
//! An [`EnumId`] is an index into one [`Enums`], valid for one compilation in one process.
//! The durable identity of an enum is a [`DefKey`](crate::DefKey) with
//! [`DefKind::Enum`](crate::DefKind); a variant's is that key **and its name**, never its
//! position and never the numeric discriminant the backend gives it.

use std::collections::HashMap;

use nazm_span::Span;

use crate::interface::Visibility;
use crate::module::ModuleId;
use crate::record::FieldDef;

/// An enum of this compilation, by declaration order. Session-local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumId(u32);

impl EnumId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// One variant: a name, its payload fields, and where each was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDef {
    pub name: String,
    pub name_span: Span,
    /// The payload fields **in declaration order**, which is what a reader wrote.
    ///
    /// Empty for a zero-payload variant. The same [`FieldDef`] a record uses, because a
    /// payload *is* record-shaped — `docs/spec.md` makes that explicit so that a future
    /// tuple variant is a new decision rather than a drift.
    pub fields: Vec<FieldDef>,
    /// Indices into [`Self::fields`], sorted by field name. The canonical order, for the
    /// same reason a record has one: an edit the language says means nothing must change
    /// no byte of the interface or of the object.
    pub layout: Vec<u32>,
}

impl VariantDef {
    /// The payload field of this name, with its index in declaration order.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(u32, &FieldDef)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name)
            .map(|(i, f)| (u32::try_from(i).unwrap_or(u32::MAX), f))
    }

    /// The payload fields in canonical order, each with its declaration index.
    pub fn in_layout_order(&self) -> impl Iterator<Item = (u32, &FieldDef)> {
        self.layout
            .iter()
            .filter_map(|i| self.fields.get(*i as usize).map(|f| (*i, f)))
    }
}

/// One enum definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub module: ModuleId,
    pub visibility: Visibility,
    pub name_span: Span,
    /// The variants **in declaration order**.
    pub variants: Vec<VariantDef>,
    /// Indices into [`Self::variants`], sorted by variant name.
    ///
    /// The **canonical** order, and therefore the order the backend assigns
    /// discriminants in. `docs/spec.md` makes variant declaration order non-semantic;
    /// deriving the tag from the names rather than from the parser is what makes that
    /// true of the emitted code as well. Taking the parser's order because it was there
    /// would have created an ABI difference from an edit with no meaning.
    pub order: Vec<u32>,
    /// This enum's index in `Program::enums`, so a consumer holding an [`EnumId`] can
    /// reach the declaration. An index into **the program this was produced from**.
    pub decl: u32,
}

impl EnumDef {
    /// The variant of this name, with its index in declaration order.
    #[must_use]
    pub fn variant(&self, name: &str) -> Option<(u32, &VariantDef)> {
        self.variants
            .iter()
            .enumerate()
            .find(|(_, v)| v.name == name)
            .map(|(i, v)| (u32::try_from(i).unwrap_or(u32::MAX), v))
    }

    /// The variants in canonical order, each with its declaration index.
    ///
    /// The position in **this** sequence is the discriminant the backend writes. It is
    /// not a semantic identity and is never persisted: `docs/spec.md` says a variant means
    /// its enum and its name, and the number is chosen fresh for each build.
    pub fn in_tag_order(&self) -> impl Iterator<Item = (u32, &VariantDef)> {
        self.order
            .iter()
            .filter_map(|i| self.variants.get(*i as usize).map(|v| (*i, v)))
    }

    /// The discriminant of the variant at this declaration index.
    #[must_use]
    pub fn tag_of(&self, declaration: u32) -> Option<u32> {
        self.order
            .iter()
            .position(|i| *i == declaration)
            .map(|t| u32::try_from(t).unwrap_or(u32::MAX))
    }
}

/// Every enum declared in one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Enums {
    defs: Vec<EnumDef>,
    /// Name to definition, per module. There is no global type namespace, and records and
    /// enums share the one a module has — the collision between `struct Item` and
    /// `enum Item` is reported by the checker, which can see both tables.
    by_module: Vec<HashMap<String, EnumId>>,
}

impl Enums {
    #[must_use]
    pub fn all(&self) -> &[EnumDef] {
        &self.defs
    }

    #[must_use]
    pub fn get(&self, id: EnumId) -> Option<&EnumDef> {
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

    /// The enum this module declares under this name — public or private, since a module
    /// sees all of its own.
    #[must_use]
    pub fn declared(&self, module: ModuleId, name: &str) -> Option<EnumId> {
        self.by_module.get(module.index())?.get(name).copied()
    }

    /// Every enum declared anywhere under this name, for diagnostics only.
    pub fn declared_anywhere<'a>(&'a self, name: &'a str) -> impl Iterator<Item = EnumId> + 'a {
        self.defs
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.name == name)
            .map(|(i, _)| EnumId::from_index(i))
    }

    /// Declare an enum with its name and visibility, before its payload types are known.
    pub fn declare(&mut self, def: EnumDef) -> EnumId {
        let id = EnumId::from_index(self.defs.len());
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

    /// Add a definition **without registering its name**, for an enum an interface carries
    /// because an exported shape mentions it. Registering it would be the transitive
    /// visibility `docs/spec.md` refuses.
    pub fn declare_unnamed(&mut self, def: EnumDef) -> EnumId {
        let id = EnumId::from_index(self.defs.len());
        self.defs.push(def);
        id
    }

    /// Give a declared enum its variants, once their payload types have been resolved.
    ///
    /// `variants` is in declaration order and each variant's `fields` likewise; both
    /// canonical orders are computed here so that no consumer has to know the rule and
    /// none can disagree about it.
    pub fn complete(&mut self, id: EnumId, mut variants: Vec<VariantDef>) {
        for v in &mut variants {
            let mut layout: Vec<u32> =
                (0..u32::try_from(v.fields.len()).unwrap_or(u32::MAX)).collect();
            layout.sort_by(|a, b| {
                let name = |i: &u32| v.fields.get(*i as usize).map_or("", |f| f.name.as_str());
                name(a).cmp(name(b))
            });
            v.layout = layout;
        }
        let mut order: Vec<u32> = (0..u32::try_from(variants.len()).unwrap_or(u32::MAX)).collect();
        order.sort_by(|a, b| {
            let name = |i: &u32| variants.get(*i as usize).map_or("", |v| v.name.as_str());
            name(a).cmp(name(b))
        });
        if let Some(def) = self.defs.get_mut(id.index()) {
            def.variants = variants;
            def.order = order;
        }
    }
}
