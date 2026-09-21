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

use crate::intrinsic::Intrinsic;
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
    pub params: Vec<Type>,
    pub ret: Type,
    pub name_span: Span,
    /// Every binding in the body, in the order slots were allocated.
    pub locals: Vec<LocalDef>,
}

/// Everything resolution decided about one compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    functions: Vec<FnDef>,
    by_name: HashMap<String, FnId>,
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

    #[must_use]
    pub fn function_id(&self, name: &str) -> Option<FnId> {
        self.by_name.get(name).copied()
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
    pub fn declare(&mut self, def: FnDef) -> FnId {
        let id = FnId::from_index(self.functions.len());
        self.by_name.insert(def.name.clone(), id);
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
