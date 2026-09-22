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

use crate::interface::Visibility;
use crate::intrinsic::Intrinsic;
use crate::module::ModuleId;
use crate::types::Type;

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
}

/// Everything resolution decided about one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    functions: Vec<FnDef>,
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
}

impl Resolution {
    #[must_use]
    pub fn functions(&self) -> &[FnDef] {
        &self.functions
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
}
