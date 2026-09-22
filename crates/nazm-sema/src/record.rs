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
//! every property a consumer wants is a method here. The alternative — copying a field
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

use crate::interface::Visibility;
use crate::module::ModuleId;
use crate::types::Type;

/// How deeply records may contain records by value.
///
/// A bound rather than a stack: computing a layout, a copy or a drop walks the
/// containment graph, and `docs/architecture.md`'s rule about recursion with the shape of
/// the input applies to *type* structure too. Cycles are already refused
/// ([`Records::layout_cycle`]), so the graph is finite — but finite is not bounded, and a
/// thousand-deep chain would still overflow whichever consumer recursed first.
///
/// 32 rather than the parser's 512, and the two measure different things: that one bounds
/// how deeply one *expression* nests, which a generator reaches easily, while this bounds
/// how deeply a *type* contains types. Each level here multiplies the copy and drop code
/// generated for every value of the outer type, so the cost is in the emitted program
/// rather than in the compiler's stack. Nothing hand-written comes near it, and a program
/// that does is refused by name rather than by a crash.
pub const MAX_RECORD_DEPTH: usize = 32;

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

/// Why a record's fields cannot be laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCycle {
    /// The records in the cycle, starting and ending at the same one.
    pub through: Vec<RecordId>,
    /// The field that closes it: `(record, index into its declaration-order fields)`.
    pub via: (RecordId, u32),
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

    // ---- derived properties ----

    /// How a type is spelled for a reader.
    ///
    /// Built-ins spell themselves; a record spells its declared name. Not an identity —
    /// two modules may each declare `Thing` and this returns `Thing` for both, which is
    /// correct for a message and wrong for anything that compares. Comparison is
    /// [`Type`]'s own `Eq`, over ids.
    #[must_use]
    pub fn show(&self, t: Type) -> &str {
        match t {
            Type::Record(id) => self.get(id).map_or("<unknown record>", |r| r.name.as_str()),
            other => other.builtin_name().unwrap_or("<unknown type>"),
        }
    }

    /// Whether a value of this type owns anything that has to be released.
    ///
    /// The one derivation `docs/architecture.md` §7.9 asks for, extended: a record needs
    /// cleanup exactly when some field does, recursively. Consumers do not each walk the
    /// fields — the interpreter, the lowerer and the emitter all ask this.
    ///
    /// Iterative rather than recursive, with an explicit worklist, because the depth is a
    /// property of the program.
    #[must_use]
    pub fn needs_cleanup(&self, t: Type) -> bool {
        let Type::Record(root) = t else {
            return !matches!(t, Type::Int | Type::Bool);
        };
        let mut seen = vec![false; self.defs.len()];
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            if seen.get(id.index()).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = seen.get_mut(id.index()) {
                *s = true;
            }
            let Some(def) = self.get(id) else { continue };
            for f in &def.fields {
                match f.ty {
                    Type::Int | Type::Bool => {}
                    Type::Record(inner) => work.push(inner),
                    // Str, Ints, Strs, Chan: every one of them owns something since N8.
                    _ => return true,
                }
            }
        }
        false
    }

    /// Whether a value of this type may be passed to a task, and if not, why.
    ///
    /// `Ok(())` or the **field path** that makes it illegal. `docs/spec.md`'s rule is
    /// that a sequence may not cross; a record inherits that from its fields rather than
    /// carrying a marker, so `Outer { inner: Inner { values: Ints } }` is refused and the
    /// diagnostic can say `inner.values`.
    ///
    /// Depth-first over the declaration order, so the path reported is the first one a
    /// reader would find.
    ///
    /// # Errors
    ///
    /// [`TaskUnsafe`] naming the field path and the type that cannot cross.
    pub fn task_safety(&self, t: Type) -> Result<(), TaskUnsafe> {
        if !matches!(t, Type::Record(_)) {
            return if t.may_cross_into_a_task() {
                Ok(())
            } else {
                Err(TaskUnsafe {
                    path: Vec::new(),
                    ty: t,
                })
            };
        }
        // An explicit stack of (record, next field to look at), so the walk is bounded by
        // the heap rather than by the host's frame size. `path` mirrors it.
        let Type::Record(root) = t else {
            return Ok(());
        };
        let mut stack: Vec<(RecordId, usize)> = vec![(root, 0)];
        let mut path: Vec<String> = Vec::new();
        while let Some((id, at)) = stack.pop() {
            let Some(def) = self.get(id) else { continue };
            let Some(f) = def.fields.get(at) else {
                path.pop();
                continue;
            };
            stack.push((id, at + 1));
            match f.ty {
                Type::Record(inner) => {
                    path.push(f.name.clone());
                    stack.push((inner, 0));
                }
                other if !other.may_cross_into_a_task() => {
                    path.push(f.name.clone());
                    return Err(TaskUnsafe { path, ty: other });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The inline-containment cycle this record is part of, if it is part of one.
    ///
    /// A record contains its fields **by value**, so a chain of record fields that
    /// returns to where it started describes a value that contains itself — no finite
    /// layout exists. `docs/spec.md` refuses it as an impossible layout rather than as a
    /// memory-safety problem, and the rule is stated over inline containment so that a
    /// future indirection can break a cycle without this rule having to change.
    ///
    /// Iterative depth-first with an explicit stack.
    #[must_use]
    pub fn layout_cycle(&self, start: RecordId) -> Option<LayoutCycle> {
        // 0 unvisited, 1 on the current path, 2 finished.
        let mut state = vec![0_u8; self.defs.len()];
        let mut path: Vec<RecordId> = Vec::new();
        let mut stack: Vec<(RecordId, usize)> = vec![(start, 0)];
        while let Some((id, at)) = stack.pop() {
            if at == 0 {
                match state.get(id.index()).copied() {
                    Some(1) => {
                        // Found it: the cycle is the suffix of `path` from this record on.
                        let from = path.iter().position(|p| *p == id)?;
                        let mut through: Vec<RecordId> = path.get(from..)?.to_vec();
                        let last = *through.last()?;
                        let via = self
                            .get(last)?
                            .fields
                            .iter()
                            .position(|f| f.ty == Type::Record(id))?;
                        through.push(id);
                        return Some(LayoutCycle {
                            through,
                            via: (last, u32::try_from(via).unwrap_or(u32::MAX)),
                        });
                    }
                    Some(2) => continue,
                    _ => {}
                }
                if let Some(s) = state.get_mut(id.index()) {
                    *s = 1;
                }
                path.push(id);
            }
            let Some(def) = self.get(id) else { continue };
            let next = def
                .fields
                .iter()
                .enumerate()
                .skip(at)
                .find_map(|(i, f)| match f.ty {
                    Type::Record(inner) => Some((i, inner)),
                    _ => None,
                });
            match next {
                Some((i, inner)) => {
                    stack.push((id, i + 1));
                    stack.push((inner, 0));
                }
                None => {
                    if let Some(s) = state.get_mut(id.index()) {
                        *s = 2;
                    }
                    path.pop();
                }
            }
        }
        None
    }

    /// How deeply this type nests records inside records, counting itself as 1.
    ///
    /// `None` when the chain is longer than [`MAX_RECORD_DEPTH`]. Only meaningful once
    /// [`Self::layout_cycle`] has found nothing — a cycle has no depth, and this would
    /// walk it forever. The caller checks cycles first, which is the order
    /// `docs/spec.md`'s declaration stages are written in.
    #[must_use]
    pub fn depth(&self, t: Type) -> Option<usize> {
        let Type::Record(root) = t else {
            return Some(0);
        };
        // (record, next field) with an explicit stack; the deepest the stack ever got is
        // the answer, and exceeding the bound stops the walk rather than the process.
        let mut stack: Vec<(RecordId, usize)> = vec![(root, 0)];
        let mut deepest = 1;
        while let Some((id, at)) = stack.pop() {
            let Some(def) = self.get(id) else { continue };
            let next = def
                .fields
                .iter()
                .enumerate()
                .skip(at)
                .find_map(|(i, f)| match f.ty {
                    Type::Record(inner) => Some((i, inner)),
                    _ => None,
                });
            if let Some((i, inner)) = next {
                stack.push((id, i + 1));
                stack.push((inner, 0));
                deepest = deepest.max(stack.len());
                if deepest > MAX_RECORD_DEPTH {
                    return None;
                }
            }
        }
        Some(deepest)
    }
}
