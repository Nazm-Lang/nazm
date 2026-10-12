//! What one module offers the modules that import it.
//!
//! # The smallest thing that is enough
//!
//! A [`UnitInterface`] is a module's identity, the definitions it exports with their
//! signatures, and the modules it directly imports. That is the whole of it, and the
//! omissions are the design:
//!
//! - **no bodies**, which is the point — a module is checked against this instead of
//!   against its dependencies' implementations;
//! - **no private definitions**, because they are not offered;
//! - no locals, no control flow, no lowering, no ownership, no effects, no layout, and no
//!   content hash. Each of those is either a later milestone or a different question.
//!
//! The test it exists to make answerable is in `docs/spec.md`: *given a module's source
//! and the interfaces of the modules it imports, that module can be checked with no
//! access to those modules' bodies.* `nazm_core::check_unit` takes exactly these.
//!
//! # What it is not, yet
//!
//! Not a file, not a cache entry, and not a thing that crosses a process boundary. It is
//! built during a compilation from the modules' own declarations and dropped with it.
//! [`Export::def`] is a session-local [`FnId`], which is precisely the field that would
//! have to become a durable identity before an interface could be written to disk and
//! read back by a later build — see `docs/research-register.md` R2.

use nazm_span::Span;

use crate::enums::EnumId;
use crate::module::{Dependency, ModuleId};
use crate::record::RecordId;
use crate::resolve::FnId;
use crate::types::Type;
use crate::udt::{TypeNode, UserTypes};

/// Whether a definition leaves its module.
///
/// One bit, and the language has one keyword for it. `docs/spec.md` states the default
/// and why there is no `private`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Nameable only inside the module that defines it. The default, written by writing
    /// nothing.
    Private,
    /// Nameable by modules that **directly** import this one. Written `pub`.
    Exported,
}

impl Visibility {
    /// Derived from whether the parser saw the word.
    ///
    /// The syntax tree records `pub_span` — that `pub` was written, and where — and stops
    /// there. This is the one place that fact becomes a meaning.
    #[must_use]
    pub const fn from_keyword(written: Option<Span>) -> Self {
        match written {
            Some(_) => Self::Exported,
            None => Self::Private,
        }
    }

    #[must_use]
    pub const fn is_exported(self) -> bool {
        matches!(self, Self::Exported)
    }
}

/// One definition a module offers by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// The trait each type parameter requires, by position (N78).
    pub bounds: Vec<(u32, String, ModuleId)>,
    pub name: String,
    pub params: Vec<Type>,
    pub ret: Type,
    /// Where the name is written in the **exporting** module.
    ///
    /// So that a diagnostic in the importing module can point at the definition it is
    /// about: "exported here", in a file the reader can open.
    pub name_span: Span,
    /// Which definition this is, in this compilation.
    ///
    /// Session-local. An interface that outlived its process would need something else
    /// here, and nothing here pretends otherwise.
    pub def: FnId,
    /// The type parameters' names, in order; empty unless the function is generic. Only
    /// the count is part of the interface — names are presentation, and a function read
    /// back from a persisted interface has none.
    pub type_params: Vec<String>,
    /// Its declared effect set (N36), or `None` where it declares none. An importer never
    /// reads the body, so `None` means **every** effect to it: what crosses a module
    /// boundary is a contract, and an absent one promises nothing.
    pub effects: Option<crate::EffectSet>,
    /// The C symbol, for a foreign declaration (N42): an importer calls it without the
    /// declaration's source, and must know a call to it needs a `ForeignCap`.
    pub foreign: Option<String>,
    /// How many `requires` clauses it states (N87): each call of it is an obligation.
    pub preconditions: u32,
    /// For a re-export (N103), the module that defines it and the name it has there — the
    /// identity a persisted interface writes. `None` for this module's own definition.
    pub origin: Option<(ModuleId, String)>,
}

/// One record definition an importer needs to see.
///
/// # Why an interface carries records it does not export
///
/// N9 records are **transparent** (`docs/spec.md`): a module that can see a record can
/// construct it and project its fields, so its field types are part of its usable shape.
/// If an exported record has a field whose type is a record from a *third* module, an
/// importer that never imported that module still has to know that record's shape to
/// project through it — and cannot, under the non-transitive import rule, go and read it.
///
/// So the interface is closed over what its public surface mentions, and [`Self::nameable`]
/// draws the line the visibility law draws: a name an importer may *write* versus a shape
/// it merely has to *understand*. Closing over shapes is not transitive visibility — no
/// extra name becomes writable — and without it an exported record could refer to a type
/// the importer could never resolve, which `docs/spec.md` refuses as an unclosed API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordExport {
    pub def: RecordId,
    /// Whether an importing module may write this name.
    ///
    /// True exactly when this module declares it and wrote `pub`. False for a record
    /// pulled in because an exported shape mentions it.
    pub nameable: bool,
}

/// One enum definition an importer needs to see.
///
/// The same rule and the same reason as [`RecordExport`], one kind over: an enum is
/// transparent too — a module that can see one can construct its variants and match on
/// them, so every payload type is part of its usable shape, and the closure carries those
/// types whether or not their own modules were imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumExport {
    pub def: EnumId,
    /// Whether an importing module may write this name. See [`RecordExport::nameable`].
    pub nameable: bool,
}

/// A module's outward face: what it is, what it offers, and what it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInterface {
    module: ModuleId,
    /// Its exported traits and every impl it declares (N78): an importer's method lookup
    /// and coherence check read both, so both are part of what the module offers.
    traits: Vec<TraitExport>,
    impls: Vec<ImplExport>,
    exports: Vec<Export>,
    /// Every record definition this interface's public surface mentions, closed over
    /// field types. See [`RecordExport`].
    records: Vec<RecordExport>,
    /// Every enum definition its public surface mentions, closed the same way.
    enums: Vec<EnumExport>,
    /// A type re-exported under another name (N103): the name an importer writes, and the type
    /// it means — the defining module's, never a copy.
    renamed: Vec<(String, Type)>,
    imports: Vec<Dependency>,
}

impl UnitInterface {
    #[must_use]
    pub fn new(
        module: ModuleId,
        exports: Vec<Export>,
        records: Vec<RecordExport>,
        enums: Vec<EnumExport>,
        imports: Vec<Dependency>,
    ) -> Self {
        Self {
            module,
            traits: Vec::new(),
            impls: Vec::new(),
            exports,
            records,
            enums,
            renamed: Vec::new(),
            imports,
        }
    }

    /// Offer a definition another module made, under `export.name` (N103). The entry keeps
    /// the definition's own id: a re-export is a second name, not a second definition.
    pub fn add_reexport(&mut self, export: Export) {
        self.exports.push(export);
    }

    /// Offer a type another module declared (N103), under its own name or — with `alias` —
    /// another. Its shape is carried as any mentioned type's is.
    pub fn add_reexported_type(&mut self, t: Type, alias: Option<String>) {
        let nameable = alias.is_none();
        match t {
            Type::Record(def) => match self.records.iter_mut().find(|r| r.def == def) {
                Some(r) => r.nameable |= nameable,
                None => self.records.push(RecordExport { def, nameable }),
            },
            Type::Enum(def) => match self.enums.iter_mut().find(|e| e.def == def) {
                Some(e) => e.nameable |= nameable,
                None => self.enums.push(EnumExport { def, nameable }),
            },
            _ => return,
        }
        if let Some(name) = alias {
            self.renamed.push((name, t));
        }
    }

    /// Carry another module's impls and traits (N103): what a re-export offers depends on
    /// them, so they are part of this interface — and of what its hash covers.
    pub fn add_reexported_traits(&mut self, traits: &[TraitExport], impls: &[ImplExport]) {
        for t in traits {
            if !self.traits.contains(t) {
                self.traits.push(t.clone());
            }
        }
        for i in impls {
            if !self.impls.contains(i) {
                self.impls.push(i.clone());
            }
        }
    }

    /// The types this interface offers under a name other than their own (N103).
    #[must_use]
    pub fn renamed(&self) -> &[(String, Type)] {
        &self.renamed
    }

    /// The same interface, offering these traits and impls (N78).
    #[must_use]
    pub fn with_traits(mut self, traits: Vec<TraitExport>, impls: Vec<ImplExport>) -> Self {
        self.traits = traits;
        self.impls = impls;
        self
    }

    #[must_use]
    pub fn traits(&self) -> &[TraitExport] {
        &self.traits
    }

    #[must_use]
    pub fn impls(&self) -> &[ImplExport] {
        &self.impls
    }

    #[must_use]
    pub fn module(&self) -> ModuleId {
        self.module
    }

    /// The exported definitions, in declaration order.
    #[must_use]
    pub fn exports(&self) -> &[Export] {
        &self.exports
    }

    /// The export with this name, if the module has one.
    ///
    /// A scan rather than a map: a module's exported surface is small by construction, and
    /// a second index over it is a second thing that can disagree with the first.
    #[must_use]
    pub fn export(&self, name: &str) -> Option<&Export> {
        self.exports.iter().find(|e| e.name == name)
    }

    /// Every record definition this interface carries, nameable or not.
    #[must_use]
    pub fn records(&self) -> &[RecordExport] {
        &self.records
    }

    /// Every enum definition this interface carries, nameable or not.
    #[must_use]
    pub fn enums(&self) -> &[EnumExport] {
        &self.enums
    }

    /// The record an importer names with this spelling, if this module offers one.
    #[must_use]
    pub fn record(&self, types: &UserTypes, name: &str) -> Option<RecordId> {
        self.records
            .iter()
            .filter(|r| r.nameable)
            .find(|r| types.records().get(r.def).is_some_and(|d| d.name == name))
            .map(|r| r.def)
            .or_else(|| {
                self.renamed.iter().find_map(|(n, t)| match t {
                    Type::Record(def) if n == name => Some(*def),
                    _ => None,
                })
            })
    }

    /// The enum an importer names with this spelling, if this module offers one.
    #[must_use]
    pub fn enumeration(&self, types: &UserTypes, name: &str) -> Option<EnumId> {
        self.enums
            .iter()
            .filter(|e| e.nameable)
            .find(|e| types.enums().get(e.def).is_some_and(|d| d.name == name))
            .map(|e| e.def)
            .or_else(|| {
                self.renamed.iter().find_map(|(n, t)| match t {
                    Type::Enum(def) if n == name => Some(*def),
                    _ => None,
                })
            })
    }

    /// The user-defined type an importer names with this spelling.
    ///
    /// One namespace: `docs/spec.md` puts records and enums both in the type namespace, so
    /// a module cannot offer both under one name and this cannot have to choose.
    #[must_use]
    pub fn user_type(&self, types: &UserTypes, name: &str) -> Option<Type> {
        self.record(types, name)
            .map(Type::Record)
            .or_else(|| self.enumeration(types, name).map(Type::Enum))
    }

    /// The modules this one directly imports.
    #[must_use]
    pub fn imports(&self) -> &[Dependency] {
        &self.imports
    }
}

/// Every user-defined type reachable from a set of exported types, including them.
///
/// The closure [`RecordExport`] and [`EnumExport`] describe, computed once here so that the
/// in-memory interface and the persisted one cannot disagree about what "closed" means.
/// One walk over records and enums together, because a record field may be an enum and an
/// enum payload may be a record — two walks would each stop at the other's edges.
/// Iterative, like everything else that walks the containment graph.
#[must_use]
pub fn reachable_types(types: &UserTypes, roots: impl IntoIterator<Item = Type>) -> Vec<TypeNode> {
    let mut seen: Vec<TypeNode> = Vec::new();
    let mut out: Vec<TypeNode> = Vec::new();
    let mut work: Vec<Type> = roots.into_iter().collect();
    work.reverse();
    while let Some(t) = work.pop() {
        match t {
            // A `Vec` is not a definition: what it reaches is its element's. Nor is a
            // function type: what it reaches is its parameters' and its result's (N50).
            Type::Vec(v) | Type::Box(v) | Type::List(v) => work.extend(types.vec_element(v)),
            Type::ChanOf(c) => work.extend(types.chan_element(c)),
            Type::Fn(_) => work.extend(types.type_args(t)),
            // A tuple (L5) is no definition either: an interface writes it as its elements.
            Type::Record(_) if types.tuple_elements(t).is_some() => {
                work.extend(types.type_args(t));
            }
            Type::Record(_) | Type::Enum(_) => {
                // An application reaches the **definition** it applies — which is what an
                // interface publishes — and everything its arguments reach.
                let mut inner: Vec<Type> = types.type_args(t);
                if let Some(node) = types.definition_of(t) {
                    if !seen.contains(&node) {
                        seen.push(node);
                        out.push(node);
                        match node {
                            TypeNode::Record(id) => {
                                if let Some(def) = types.records().get(id) {
                                    inner.extend(def.fields.iter().map(|f| f.ty));
                                }
                            }
                            TypeNode::Enum(id) => {
                                if let Some(def) = types.enums().get(id) {
                                    for v in &def.variants {
                                        inner.extend(v.fields.iter().map(|f| f.ty));
                                    }
                                }
                            }
                        }
                    }
                }
                for n in inner.into_iter().rev() {
                    work.push(n);
                }
            }
            _ => {}
        }
    }
    out
}

/// An exported trait, as an interface offers it (N78): its name, and each method's
/// signature after `self`, with `Self` the trait's parameter 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitExport {
    pub name: String,
    pub module: ModuleId,
    pub methods: Vec<(
        String,
        Vec<crate::types::Type>,
        crate::types::Type,
        crate::EffectSet,
    )>,
}

/// An impl a module declares (N78): which trait, for which type, and its methods' names in
/// the trait's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplExport {
    /// The module that declares the impl, whose identities its methods are — another module's
    /// when a re-export carries it (N103).
    pub module: ModuleId,
    pub trait_name: String,
    pub trait_module: ModuleId,
    pub self_ty: crate::types::Type,
    pub methods: Vec<String>,
}
