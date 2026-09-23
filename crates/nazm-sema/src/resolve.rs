//! What resolution decided, as a value both consumers read.
//!
//! # Why this exists
//!
//! Name resolution used to happen twice. `nazm_core::check` resolved every name and
//! returned a signature map; the driver threw it away; and `nazm_lir::lower` rebuilt an
//! equivalent one — its own function table, its own scope stack, its own slot allocator —
//! because `xtask/src/rules.rs` forbids the backend depending on the interpreter's crate.
//! Two implementations of one rule, kept in agreement by tests rather than by
//! construction.
//!
//! This is the agreement, written down once. The **checker produces it** and the
//! **backend consumes it**; the backend no longer decides what a name means.
//!
//! # What it is not
//!
//! Not an IR. It holds no expressions and no control flow — only the answers to "what does
//! this name refer to", keyed by where the name was written. Lowering still lowers; it has
//! simply stopped re-deciding.
//!
//! Not an incremental or cached artefact either. Every id here is **session-local**, in the
//! same sense as [`FileId`](nazm_span::FileId): an index, valid for one compilation in one
//! process, not derived from content and not a cache key. See `docs/research-register.md`
//! R2 and R3 for what a durable definition identity would have to be; this is deliberately
//! not that.

use std::collections::HashMap;

use nazm_span::Span;

use crate::enums::{EnumId, Enums};
use crate::interface::Visibility;
use crate::intrinsic::Intrinsic;
use crate::key::{DefKey, DefKind, ModuleKey};
use crate::module::ModuleId;
use crate::record::{RecordId, Records};
use crate::types::Type;
use crate::udt::UserTypes;

/// A function of this compilation, by position in declaration order.
///
/// Session-local. Two runs over the same tree may number functions differently and both
/// are correct; nothing may persist one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnId(u32);

impl FnId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// What a call resolved to.
///
/// Built-ins and user functions share one namespace — a call is a call — so this is the
/// one place the distinction is drawn, rather than at each consumer with its own copy of
/// the rule that built-ins are looked up first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callee {
    Builtin(Intrinsic),
    Function(FnId),
}

/// One local binding: a parameter or a `let`.
///
/// `slot` is its index in the function's storage. **Parameters are the first slots, in
/// declaration order**, and a slot is never reused — two bindings live at once must not
/// share storage, and "is the old one still live?" is a question no consumer should have
/// to ask. A name that shadows another gets a new slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDef {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
}

/// One function's resolved shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDef {
    pub name: String,
    /// The module that owns this definition.
    ///
    /// Every definition has exactly one, which is what makes a name unique without being
    /// unique in the program: two modules may each define `helper` and they are two
    /// definitions. Before N2 there was nothing to put here, because there was one
    /// namespace and a name identified a definition on its own.
    pub module: ModuleId,
    /// Whether modules that import this one may name it.
    pub visibility: Visibility,
    pub params: Vec<Type>,
    pub ret: Type,
    pub name_span: Span,
    /// Where the body is: this function's index in `Program::functions`.
    ///
    /// Carried because the syntax tree is still the only representation a body has —
    /// there is no HIR, deliberately — so a consumer holding an [`FnId`] needs a way back
    /// to the declaration it came from. It is an index into **the program this resolution
    /// was produced from** and means nothing against any other.
    pub decl: u32,
    /// Every binding in the body, in the order slots were allocated.
    pub locals: Vec<LocalDef>,
    /// The type parameters' names, in the order declared. Empty unless the function is
    /// generic. Presentation only — see [`crate::TypeParam`].
    pub type_params: Vec<String>,
    /// Every call its body makes, with the type arguments the checker settled.
    ///
    /// What the native backend's instance planner walks: which instances a body needs is a
    /// fact about the resolved body, and recording it here means planning neither re-walks
    /// the tree nor re-decides a single call. For a generic body the arguments may mention
    /// its own parameters, which is exactly what substitution then replaces.
    pub call_sites: Vec<CallSite>,
    /// Every type a **generic** body's resolution mentions — its locals, what it constructs
    /// and the type arguments of what it calls — so that an instance can have each of them
    /// interned under its substitution before any native code is written. Empty for a
    /// function that is not generic, which needs no substitution.
    pub mentions: Vec<Type>,
}

/// One call in a body: where it is, what it resolved to, and with which type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The callee's name span — the key [`Resolution::callee`] answers.
    pub span: Span,
    pub callee: Callee,
    /// The callee's type arguments, in its parameters' order. Empty for a callee that is
    /// not generic.
    pub type_args: Vec<Type>,
    /// Whether this is a `spawn` rather than a call. Both are edges of the call graph.
    pub spawned: bool,
}

/// Which field of which record a projection refers to.
///
/// **Not an offset.** `docs/architecture.md` requires field access to stay symbolic until
/// the layout boundary, so what resolution decides is *which field* — a record identity
/// and the field's index in declaration order — and the backend decides where that field
/// sits. A semantic stage that recorded a byte offset here would have made the layout
/// part of the language's meaning, and no later layout work could change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldRef {
    pub record: RecordId,
    /// The field's index in the record's **declaration order**, which is the session-local
    /// spelling of `(record definition, field name)`. Never persisted: a durable form
    /// names the field, because declaration order is not part of what a record means.
    pub field: u32,
}

/// Which variant of which enum a construction or a pattern names.
///
/// **Not a discriminant.** The number the backend writes is chosen from the canonical
/// order at lowering; what resolution decides is *which variant*, spelled as the enum's
/// identity and the variant's index in declaration order. A semantic stage that recorded a
/// tag would have made the representation part of the language's meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantRef {
    pub enumeration: EnumId,
    /// The variant's index in the enum's **declaration order**, which is the session-local
    /// spelling of `(enum definition, variant name)`. Never persisted.
    pub variant: u32,
}

/// Which payload field of which variant an initialiser or a pattern field names.
///
/// The enum's answer to [`FieldRef`], and not an offset for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadRef {
    pub enumeration: EnumId,
    pub variant: u32,
    /// The field's index in the variant's declaration order.
    pub field: u32,
}

/// Everything resolution decided about one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    functions: Vec<FnDef>,
    /// Every record and enum declared in this compilation.
    ///
    /// Here rather than beside it, for N1's reason: a user type's fields and the properties
    /// derived from them are resolution's answers, and a backend that built its own table
    /// would be a second resolver. `nazm-lir` reads this and never looks a type name up.
    types: UserTypes,
    /// Name to definition, **per module**.
    ///
    /// One map per module, indexed by [`ModuleId`]. Before N2 this was a single
    /// `HashMap<String, FnId>` over the whole program, which is the global namespace N2
    /// removed: it could not represent two modules each defining `helper`, and it gave
    /// the same answer to "what does `parse` mean" no matter who was asking.
    by_module: Vec<HashMap<String, FnId>>,
    /// Keyed by the span of the **callee name**, which is written once per call site.
    calls: HashMap<Span, Callee>,
    /// Keyed by the span of the variable mention, which is written once per use.
    vars: HashMap<Span, u32>,
    /// Keyed by the span of the **field name** in a projection or a field assignment,
    /// which is written once per access.
    fields: HashMap<Span, FieldRef>,
    /// Keyed by the span of the **type name** in a record construction.
    constructions: HashMap<Span, RecordId>,
    /// Keyed by the span of the **variant name** in a variant construction or a pattern.
    ///
    /// One map for both, because they are the same question — *which variant is this* —
    /// asked in two syntactic positions, and two maps would be two chances to disagree.
    variants: HashMap<Span, VariantRef>,
    /// Keyed by the span of a **payload field name** in a variant construction or a
    /// pattern.
    payloads: HashMap<Span, PayloadRef>,
    /// What each generic call site's type arguments were settled to, by the callee's name
    /// span — written or inferred, the checker decided and the backend reads.
    type_args: HashMap<Span, Vec<Type>>,
}

impl Resolution {
    #[must_use]
    pub fn functions(&self) -> &[FnDef] {
        &self.functions
    }

    /// Every user-defined type this compilation declared.
    #[must_use]
    pub fn types(&self) -> &UserTypes {
        &self.types
    }

    /// The user-type tables, for the checker that is building them.
    pub fn types_mut(&mut self) -> &mut UserTypes {
        &mut self.types
    }

    /// Every record this compilation declared.
    #[must_use]
    pub fn records(&self) -> &Records {
        self.types.records()
    }

    /// The record table, for the checker that is building it.
    pub fn records_mut(&mut self) -> &mut Records {
        self.types.records_mut()
    }

    /// Every enum this compilation declared.
    #[must_use]
    pub fn enums(&self) -> &Enums {
        self.types.enums()
    }

    /// The enum table, for the checker that is building it.
    pub fn enums_mut(&mut self) -> &mut Enums {
        self.types.enums_mut()
    }

    /// What the projection whose field name was written at `span` refers to.
    #[must_use]
    pub fn field(&self, span: Span) -> Option<FieldRef> {
        self.fields.get(&span).copied()
    }

    /// Which record the construction whose type name was written at `span` builds.
    #[must_use]
    pub fn construction(&self, span: Span) -> Option<RecordId> {
        self.constructions.get(&span).copied()
    }

    /// Which variant the construction or pattern whose variant name is at `span` names.
    #[must_use]
    pub fn variant(&self, span: Span) -> Option<VariantRef> {
        self.variants.get(&span).copied()
    }

    /// Which payload field the initialiser or pattern field at `span` names.
    #[must_use]
    pub fn payload(&self, span: Span) -> Option<PayloadRef> {
        self.payloads.get(&span).copied()
    }

    #[must_use]
    pub fn function(&self, id: FnId) -> Option<&FnDef> {
        self.functions.get(id.index())
    }

    /// The definition this module makes under this name — public or private, since a
    /// module sees all of its own.
    ///
    /// Takes the asking module because the answer depends on it. That is the whole of
    /// what changed in N2: a name does not mean something, it means something *to
    /// somebody*.
    #[must_use]
    pub fn definition(&self, module: ModuleId, name: &str) -> Option<FnId> {
        self.by_module.get(module.index())?.get(name).copied()
    }

    /// Which logical record this is, across processes. The same rule as
    /// [`Self::def_key`], with [`DefKind::Record`].
    #[must_use]
    pub fn record_key(&self, id: RecordId, module: &ModuleKey) -> Option<DefKey> {
        let def = self.types.records().get(id)?;
        Some(DefKey::new(
            module.clone(),
            DefKind::Record,
            def.name.clone(),
        ))
    }

    /// Which logical enum this is, across processes. The same rule, with
    /// [`DefKind::Enum`]. A *variant* has no key of its own: `docs/spec.md` makes its
    /// durable meaning this key **and its name**, and nothing persistent has needed a
    /// composed identity, so none was invented.
    #[must_use]
    pub fn enum_key(&self, id: EnumId, module: &ModuleKey) -> Option<DefKey> {
        let def = self.types.enums().get(id)?;
        Some(DefKey::new(module.clone(), DefKind::Enum, def.name.clone()))
    }

    /// Which logical definition this is, across processes.
    ///
    /// Needs the owning module's durable key, which the caller has and this does not: a
    /// `Resolution` knows which module owns a definition and nothing about where that
    /// module's source sits. `None` when the module has no durable identity, which is an
    /// answer rather than a failure — see `docs/spec.md`, *Durable identity*.
    #[must_use]
    pub fn def_key(&self, id: FnId, module: &ModuleKey) -> Option<DefKey> {
        let def = self.function(id)?;
        Some(DefKey::new(
            module.clone(),
            DefKind::Function,
            def.name.clone(),
        ))
    }

    /// The program's entry point: `main` in the root module.
    ///
    /// Asked for rather than looked up by name, so that neither the evaluator nor the
    /// backend has to know the spelling or the rule. Both used to search their own table
    /// for "main"; both now ask the resolution, which is the only place that knows which
    /// module is the root.
    #[must_use]
    pub fn entry(&self) -> Option<FnId> {
        self.definition(ModuleId::ROOT, "main")
    }

    /// Every definition of `name` anywhere in the compilation, with its module.
    ///
    /// For diagnostics only, and for one diagnostic in particular: when a module names
    /// something it cannot see, "not defined" is true and useless if the definition is
    /// sitting in a file the reader just imported. Nothing about resolution may consult
    /// this — a resolver that looked at every module would be the global namespace again.
    pub fn defined_anywhere<'a>(&'a self, name: &'a str) -> impl Iterator<Item = FnId> + 'a {
        self.functions
            .iter()
            .enumerate()
            .filter(move |(_, f)| f.name == name)
            .map(|(i, _)| FnId::from_index(i))
    }

    /// What the call whose callee was written at `span` refers to.
    #[must_use]
    pub fn callee(&self, span: Span) -> Option<Callee> {
        self.calls.get(&span).copied()
    }

    /// Which slot the variable mentioned at `span` refers to.
    #[must_use]
    pub fn var(&self, span: Span) -> Option<u32> {
        self.vars.get(&span).copied()
    }

    // ---- construction, for the checker ----

    /// Declare a function and return its id.
    ///
    /// The name is recorded against the declaring module, not against the program. A
    /// second definition of the same name **in the same module** is the caller's error to
    /// report; this keeps the first, which is what makes one mistake stay one error.
    pub fn declare(&mut self, def: FnDef) -> FnId {
        let id = FnId::from_index(self.functions.len());
        let m = def.module.index();
        if self.by_module.len() <= m {
            self.by_module.resize_with(m + 1, HashMap::new);
        }
        if let Some(names) = self.by_module.get_mut(m) {
            names.entry(def.name.clone()).or_insert(id);
        }
        self.functions.push(def);
        id
    }

    /// Record a local, returning its slot.
    ///
    /// Returns `None` for an id this resolution does not have, which cannot happen while
    /// the checker drives it and is not worth a panic if it ever does.
    pub fn bind(&mut self, id: FnId, def: LocalDef) -> Option<u32> {
        let f = self.functions.get_mut(id.index())?;
        let slot = u32::try_from(f.locals.len()).unwrap_or(u32::MAX);
        f.locals.push(def);
        Some(slot)
    }

    pub fn record_call(&mut self, span: Span, callee: Callee) {
        self.calls.insert(span, callee);
    }

    pub fn record_var(&mut self, span: Span, slot: u32) {
        self.vars.insert(span, slot);
    }

    pub fn record_field(&mut self, span: Span, to: FieldRef) {
        self.fields.insert(span, to);
    }

    pub fn record_construction(&mut self, span: Span, of: RecordId) {
        self.constructions.insert(span, of);
    }

    pub fn record_variant(&mut self, span: Span, to: VariantRef) {
        self.variants.insert(span, to);
    }

    pub fn record_payload(&mut self, span: Span, to: PayloadRef) {
        self.payloads.insert(span, to);
    }

    /// Record the type arguments a generic call site was checked with.
    pub fn record_type_args(&mut self, span: Span, args: Vec<Type>) {
        self.type_args.insert(span, args);
    }

    /// The type arguments a generic call site was checked with, if it had any.
    #[must_use]
    pub fn type_args(&self, span: Span) -> Option<&[Type]> {
        self.type_args.get(&span).map(Vec::as_slice)
    }

    /// Record a call a function's body makes.
    pub fn record_call_site(&mut self, id: FnId, site: CallSite) {
        if let Some(f) = self.functions.get_mut(id.index()) {
            f.call_sites.push(site);
        }
    }

    /// Record that a generic function's body mentions a type.
    pub fn record_mention(&mut self, id: FnId, t: Type) {
        if let Some(f) = self.functions.get_mut(id.index()) {
            if !f.type_params.is_empty() && !f.mentions.contains(&t) {
                f.mentions.push(t);
            }
        }
    }

    /// Give a declared function its signature, once its types have been resolved.
    ///
    /// Two steps, like a record's fields, because a generic function's parameter types
    /// mention its own parameters — and a parameter names its owner, which needs the
    /// function's id before anything else about it can be written down.
    pub fn complete_signature(&mut self, id: FnId, params: Vec<Type>, ret: Type) {
        if let Some(f) = self.functions.get_mut(id.index()) {
            f.params = params;
            f.ret = ret;
        }
    }

    /// The id the next [`Self::declare`] will return.
    #[must_use]
    pub fn next_function(&self) -> FnId {
        FnId::from_index(self.functions.len())
    }
}
