//! Traits and their impls (N78, `docs/architecture.md` §7.79): the checker's one table of
//! which type implements which trait, and with which functions.
//!
//! A trait is a named set of method signatures in which `Self` is the trait's own type
//! parameter (`ParamOwner::Trait`, position 0), so substituting an impl's type for it is the
//! substitution generics already perform. An impl is a trait, a concrete type, and one
//! function per trait method, in the trait's order. Session-local ids, as every other id in
//! this crate; durable keys are the caller's (`DefKind::Trait`, and `Trait/Type/m` for an
//! impl's function).

use std::collections::BTreeMap;

// Keyed by spans and by trait and type, per check: the fast hasher (N106).
use nazm_span::FastMap as HashMap;

use nazm_span::Span;

use crate::effect::EffectSet;
use crate::interface::Visibility;
use crate::module::ModuleId;
use crate::resolve::FnId;
use crate::types::{ParamOwner, Type, TypeParam};

/// A trait of this compilation, by declaration order. Session-local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraitId(u32);

impl TraitId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }

    /// The trait's `Self`: its own type parameter.
    #[must_use]
    pub const fn self_param(self) -> TypeParam {
        TypeParam {
            owner: ParamOwner::Trait(self),
            index: 0,
        }
    }
}

/// An impl of this compilation, by declaration order. Session-local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplId(u32);

impl ImplId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One method a trait requires. `params` are the parameters after `self`, whose type is
/// always the trait's `Self`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethodDef {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Type>,
    pub ret: Type,
    /// The declared effect set; `None` was written as none, which for a trait method means
    /// the empty set — a trait's signature is a promise, never an inference.
    pub effects: EffectSet,
    /// Whether the trait writes a default body (R1b, L11), declared as the function
    /// `Trait/default/m` generic over `Self` ([`crate::Resolution::default_method`]).
    pub has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDef {
    pub name: String,
    pub module: ModuleId,
    pub visibility: Visibility,
    pub name_span: Span,
    pub methods: Vec<TraitMethodDef>,
}

impl TraitDef {
    /// The method of this name, by its position.
    #[must_use]
    pub fn method(&self, name: &str) -> Option<u32> {
        self.methods
            .iter()
            .position(|m| m.name == name)
            .and_then(|i| u32::try_from(i).ok())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDef {
    pub trait_id: TraitId,
    pub self_ty: Type,
    pub module: ModuleId,
    /// The `impl` keyword.
    pub impl_span: Span,
    /// The function for each trait method, by the trait's method order; `None` where the
    /// impl did not define one, which checking has refused.
    pub methods: Vec<Option<FnId>>,
}

/// A method call resolved through a trait, where the receiver's type is a type parameter
/// and the impl is chosen per instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitCall {
    pub trait_id: TraitId,
    pub method: u32,
    /// The receiver's type as the calling body sees it: a type parameter of that body.
    pub self_ty: Type,
}

/// Every trait and impl of the compilation, and the bounds type parameters carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Traits {
    traits: Vec<TraitDef>,
    impls: Vec<ImplDef>,
    /// The coherence table: the one impl of a trait for a type.
    table: HashMap<(TraitId, Type), ImplId>,
    /// What each type parameter requires beyond equality.
    bounds: BTreeMap<TypeParam, TraitId>,
    /// Method calls through a bound, by the method name's span.
    calls: HashMap<Span, TraitCall>,
    /// Every place a trait is named: a bound, an impl's header, a call that names it.
    mentions: HashMap<Span, TraitId>,
    /// Traits a module offers that another module declared (N103): `pub use` re-exports.
    reexported: Vec<(crate::module::ModuleId, TraitId)>,
    /// Which modules each module re-exports from (N103): their impls are usable wherever the
    /// re-exporting module is imported, as the module's own are.
    sources: Vec<(crate::module::ModuleId, crate::module::ModuleId)>,
    /// `@std/hash`'s `Hash`, when the compilation has it (L6): the trait a record, enum or
    /// tuple satisfies without an impl when its components do.
    derived_hash: Option<TraitId>,
}

impl Traits {
    /// Name `@std/hash`'s `Hash` as the derivable trait (L6).
    pub fn set_derived_hash(&mut self, id: Option<TraitId>) {
        self.derived_hash = id;
    }

    /// `@std/hash`'s `Hash`, when the compilation has it (L6).
    #[must_use]
    pub const fn derived_hash(&self) -> Option<TraitId> {
        self.derived_hash
    }

    /// Whether `ty` satisfies `trait_id` by derivation rather than by an impl (L6,
    /// `docs/spec.md`, *Ordering and hashing are derived*): the trait is `@std/hash`'s `Hash`,
    /// `ty` has no impl of its own, and every component is an integer, `Bool`, `Str`, or a
    /// record, enum or tuple of them none of which has an impl.
    #[must_use]
    pub fn derives(&self, trait_id: TraitId, ty: Type, types: &crate::UserTypes) -> bool {
        self.derived_hash == Some(trait_id)
            && self.find(trait_id, ty).is_none()
            && types
                .hashing(ty, |c| self.find(trait_id, c).is_some())
                .is_ok()
    }

    pub fn declare(&mut self, def: TraitDef) -> TraitId {
        let id = TraitId::from_index(self.traits.len());
        self.traits.push(def);
        id
    }

    #[must_use]
    pub fn get(&self, id: TraitId) -> Option<&TraitDef> {
        self.traits.get(id.index())
    }

    pub fn get_mut(&mut self, id: TraitId) -> Option<&mut TraitDef> {
        self.traits.get_mut(id.index())
    }

    pub fn iter(&self) -> impl Iterator<Item = (TraitId, &TraitDef)> {
        self.traits
            .iter()
            .enumerate()
            .map(|(i, t)| (TraitId::from_index(i), t))
    }

    /// Record an impl. `Err` with the impl already filed under the same trait and type —
    /// the coherence violation, which the caller reports; the new one is still kept, so its
    /// methods are checked, but it is never the table's answer.
    pub fn implement(&mut self, def: ImplDef) -> Result<ImplId, ImplId> {
        let id = ImplId(u32::try_from(self.impls.len()).unwrap_or(u32::MAX));
        let key = (def.trait_id, def.self_ty);
        self.impls.push(def);
        match self.table.get(&key) {
            Some(first) => Err(*first),
            None => {
                self.table.insert(key, id);
                Ok(id)
            }
        }
    }

    #[must_use]
    pub fn impl_def(&self, id: ImplId) -> Option<&ImplDef> {
        self.impls.get(id.index())
    }

    pub fn impl_mut(&mut self, id: ImplId) -> Option<&mut ImplDef> {
        self.impls.get_mut(id.index())
    }

    pub fn impls(&self) -> impl Iterator<Item = (ImplId, &ImplDef)> {
        self.impls
            .iter()
            .enumerate()
            .map(|(i, d)| (ImplId(u32::try_from(i).unwrap_or(u32::MAX)), d))
    }

    /// The impl of `trait_id` for exactly `ty`, if the compilation has one.
    #[must_use]
    pub fn find(&self, trait_id: TraitId, ty: Type) -> Option<ImplId> {
        self.table.get(&(trait_id, ty)).copied()
    }

    /// The function implementing method `method` of `trait_id` for `ty`.
    #[must_use]
    pub fn method_fn(&self, trait_id: TraitId, ty: Type, method: u32) -> Option<FnId> {
        let imp = self.impl_def(self.find(trait_id, ty)?)?;
        imp.methods.get(method as usize).copied().flatten()
    }

    pub fn bound(&mut self, param: TypeParam, trait_id: TraitId) {
        self.bounds.insert(param, trait_id);
    }

    #[must_use]
    pub fn bound_of(&self, param: TypeParam) -> Option<TraitId> {
        self.bounds.get(&param).copied()
    }

    pub fn record_call(&mut self, at: Span, call: TraitCall) {
        self.calls.insert(at, call);
    }

    #[must_use]
    pub fn call(&self, at: Span) -> Option<TraitCall> {
        self.calls.get(&at).copied()
    }

    pub fn trait_calls(&self) -> impl Iterator<Item = (Span, &TraitCall)> {
        self.calls.iter().map(|(s, c)| (*s, c))
    }

    pub fn record_mention(&mut self, at: Span, trait_id: TraitId) {
        self.mentions.insert(at, trait_id);
    }

    pub fn mentions(&self) -> impl Iterator<Item = (Span, TraitId)> + '_ {
        self.mentions.iter().map(|(s, t)| (*s, *t))
    }

    /// Record that `module` re-exports `trait_id` (N103), once.
    pub fn record_reexport(&mut self, module: crate::module::ModuleId, trait_id: TraitId) {
        if !self.reexported.contains(&(module, trait_id)) {
            self.reexported.push((module, trait_id));
        }
    }

    /// Record that `module` re-exports from `from` (N103), once.
    pub fn record_reexport_source(
        &mut self,
        module: crate::module::ModuleId,
        from: crate::module::ModuleId,
    ) {
        if !self.sources.contains(&(module, from)) {
            self.sources.push((module, from));
        }
    }

    /// The modules `module` re-exports from (N103).
    pub fn reexport_sources(
        &self,
        module: crate::module::ModuleId,
    ) -> impl Iterator<Item = crate::module::ModuleId> + '_ {
        self.sources
            .iter()
            .filter(move |(m, _)| *m == module)
            .map(|(_, f)| *f)
    }

    /// The traits `module` re-exports (N103), under their own names.
    pub fn reexported_by(
        &self,
        module: crate::module::ModuleId,
    ) -> impl Iterator<Item = TraitId> + '_ {
        self.reexported
            .iter()
            .filter(move |(m, _)| *m == module)
            .map(|(_, t)| *t)
    }
}
