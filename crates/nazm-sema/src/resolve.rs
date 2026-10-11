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

// The resolver's tables are keyed by spans, filled and read once per check: the fast hasher
// (N106, `nazm_span::fast`), not SipHash.
use nazm_span::FastMap as HashMap;

use nazm_span::Span;

use crate::enums::{EnumId, Enums};
use crate::interface::Visibility;
use crate::intrinsic::Intrinsic;
use crate::key::{DefKey, DefKind, ModuleKey};
use crate::module::ModuleId;
use crate::record::{RecordId, Records};
use crate::types::{Type, TypeParam};
use crate::udt::{TypeNode, UserTypes};

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
    /// A call through a function value (N50): which function is decided at run time, and
    /// what the call may do is its function type's — parameters, result and effects.
    Indirect(Type),
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
    /// Where the binding's name is written: the `let`, the parameter or the pattern field
    /// that introduced it. N16, so a use can be taken to its definition by the identity
    /// resolution already gave it — the slot — rather than by its spelling. Presentation
    /// only: nothing about what the binding means depends on where it was written.
    pub span: Span,
}

/// Which binding a variable mention refers to: a slot **of a particular function**.
///
/// A slot alone is a position in one function's storage, so two functions' slot 0 are two
/// bindings. N17 records the owner beside it, at the point the checker already knows it,
/// so that a consumer asking "which binding is this" does not have to work out which body
/// a span sits in. Session-local, like everything else here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalRef {
    pub function: FnId,
    pub slot: u32,
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
    /// Each parameter's name, in order, as its declaration wrote it — known from the
    /// signature alone, so a module whose body was not checked (its check reused) still names
    /// its parameters where [`Self::locals`] is empty (v1, Q1-M-08).
    pub param_names: Vec<String>,
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
    /// The effect set its declaration writes (`! { … }`, N36), or `None` where none is
    /// written — or where one names an effect that does not exist, which is reported once
    /// and then treated as unwritten. A declared set is the function's contract: callers
    /// read it, and its body is checked against it.
    pub declared_effects: Option<crate::EffectSet>,
    /// What its body requires, the least fixed point over the module's calls: `None` until
    /// the body is checked, and for a body that did not parse.
    pub inferred_effects: Option<crate::EffectSet>,
    /// For each inferred effect, the first thing in the body that requires it — in effect
    /// id order, one each. A diagnostic's witness is a chain of these.
    pub effect_reasons: Vec<crate::EffectReason>,
    /// Its solved provenance (N38): what its result carries and which parameters reach a
    /// restricted sink. `None` until the program is solved, and for a body that did not
    /// parse.
    pub provenance: Option<crate::Summary>,
    /// Every type a **generic** body's resolution mentions — its locals, what it constructs
    /// and the type arguments of what it calls — so that an instance can have each of them
    /// interned under its substitution before any native code is written. Empty for a
    /// function that is not generic, which needs no substitution.
    pub mentions: Vec<Type>,
    /// The C symbol a foreign declaration names (N42), or `None` for a Nazm function. A
    /// foreign function has no body; its declared effect set is `{ foreign }`.
    pub foreign: Option<String>,
    /// The C symbol an exported function is also callable as (N55), or `None`. An export has a
    /// Nazm body and is called by Nazm as any function is.
    pub export: Option<String>,
    /// How many `requires` clauses it states (N87).
    pub preconditions: u32,
    /// For a closure (N50): the function whose body wrote it, where, and what it captured.
    /// A closure has no declaration of its own — `decl` is its enclosing function's — and
    /// no name a program can write.
    pub closure: Option<ClosureInfo>,
}

/// What makes a definition a closure (N50).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureInfo {
    /// The function or closure whose body the closure expression is in.
    pub enclosing: FnId,
    /// The closure expression's span, which is how its syntax is found again.
    pub at: Span,
    /// The span of its written effect set, if it wrote one; unwritten, it is `! {}`.
    pub written_effects: Option<Span>,
    /// Its type: `fn(params) -> ret ! {declared}`.
    pub ty: Type,
    /// Each binding it captures, in order of first mention: the enclosing function's slot,
    /// and the closure's own slot the copy lives in.
    pub captures: Vec<Capture>,
}

/// One captured binding (N50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capture {
    /// The binding in the enclosing function.
    pub outer: u32,
    /// The closure's slot holding the copy.
    pub inner: u32,
    pub ty: Type,
}

/// A named function written as a value (N50), and the effects its value's type assumes:
/// its declared set, or none for an undeclared function, which effects analysis then holds
/// to that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FnValueUse {
    pub function: FnId,
    pub at: Span,
    pub assumed: crate::EffectSet,
    /// The function whose body made the value, and the authority held where it did (N51):
    /// making a value of an undeclared function needs what calling it would.
    pub within: Option<FnId>,
    pub held: crate::CapabilitySet,
    /// The value's type: the function's signature, with `assumed` as its effects.
    pub ty: Type,
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
    /// The authority in scope at the call (N37): the kind of every capability-typed
    /// binding — parameter or `let` — a name at this point reaches. Lexical, read from the
    /// bindings a reader can see, and nothing else: no function carries authority that is
    /// not a value it holds.
    pub held: crate::CapabilitySet,
}

/// One call as the checker checked it (N20): what it resolved to, where its arguments
/// are, and the signature the arguments were checked against.
///
/// Recorded at the point the checker has all of it — after the callee resolved, whether or
/// not the arguments then fitted — so that a tool asking "which call is this, with which
/// signature" reads the answer instead of resolving the callee again or re-running
/// inference. A call whose callee did not resolve is not here: nothing was checked against
/// anything.
///
/// Not a call graph and not for the backends, which read [`FnDef::call_sites`]; this is
/// the signature side of a call, for tooling. Session-local, like everything here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedCall {
    pub callee: Callee,
    /// The whole call, from the callee's name to the `)` — or, for a `spawn`, the
    /// statement.
    pub span: Span,
    /// Each argument that parsed, in order.
    pub args: Vec<Span>,
    /// The parameter types the callee declares, exactly as the checker read them from the
    /// module's environment: a function's own or imported signature, or a built-in's. A
    /// generic callee's mention its own parameters.
    pub params: Vec<Type>,
    pub ret: Type,
    /// For a generic callee, what this call settled. `None` for one that is not generic.
    pub instance: Option<Instantiation>,
    /// Whether this is a `spawn` rather than a call.
    pub spawned: bool,
    /// How many `requires` clauses the callee states (N87): each call of one with any is an
    /// obligation, whether the callee is this module's or another's.
    pub preconditions: u32,
}

/// What one call of a generic callee settled its type parameters to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    /// Whose parameters [`CheckedCall::params`] mention.
    pub owner: crate::types::ParamOwner,
    /// Whether the type arguments were written (`f[Int](…)`) rather than inferred.
    pub written: bool,
    /// Each type parameter's argument, in declaration order — `None` where nothing settled
    /// it, which the checker has reported.
    pub args: Vec<Option<Type>>,
    /// Each parameter's type with the settled arguments in place. An unsettled parameter
    /// stays itself; `None` only where that type was never formed in this compilation.
    pub params: Vec<Option<Type>>,
    /// The result type with the settled arguments in place, on the same terms.
    pub ret: Option<Type>,
}

/// Which field of which record a projection refers to.
///
/// **Not an offset.** `docs/architecture.md` requires field access to stay symbolic until
/// the layout boundary, so what resolution decides is *which field* — a record identity
/// and the field's index in declaration order — and the backend decides where that field
/// sits. A semantic stage that recorded a byte offset here would have made the layout
/// part of the language's meaning, and no later layout work could change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantRef {
    pub enumeration: EnumId,
    /// The variant's index in the enum's **declaration order**, which is the session-local
    /// spelling of `(enum definition, variant name)`. Never persisted.
    pub variant: u32,
}

/// What one `?` was resolved to: the `Result` it takes apart, and the one it returns.
///
/// Decided once by the checker, which is the only place the operand was found to be the
/// core prelude's `Result` — by definition identity, never by a name or by the shape of
/// its variants. Both backends read this and neither asks the question again.
///
/// Two enums, not one, because they are two instances of one definition: `r?` on a
/// `Result[Int, E]` inside a function returning `Result[Str, E]` takes apart the first and
/// builds the second. Variant indices are in declaration order, like [`VariantRef`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Propagation {
    /// The operand's type: an instance of the core `Result`.
    pub operand: EnumId,
    /// Which of its variants is the value that `?` yields.
    pub ok: u32,
    /// Which of its variants is the error value that leaves.
    pub err: u32,
    /// The enclosing function's return type: an instance of the same `Result`, with the
    /// same error type.
    pub into: EnumId,
    /// Which of *its* variants the error value is returned in.
    pub into_err: u32,
}

/// Which payload field of which variant an initialiser or a pattern field names.
///
/// The enum's answer to [`FieldRef`], and not an offset for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PayloadRef {
    pub enumeration: EnumId,
    pub variant: u32,
    /// The field's index in the variant's declaration order.
    pub field: u32,
}

/// A numeric decision the checker made (Gate 2), for Core IR lowering to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericFact {
    /// A literal's type where it is not the default — `Int` for an integer, `Float64` for a
    /// floating-point literal — because a type was expected (`docs/general-purpose.md` §2).
    Literal(Type),
    /// A conversion call, by its callee's span: `T(x)`, `try_convert[T](x)`,
    /// `wrap_convert[T](x)` or `saturate_convert[T](x)`.
    Convert {
        to: Type,
        from: Type,
        mode: crate::types::ConvertMode,
        /// What the call produces: `to`, or `Option[to]` for `try_convert`.
        result: Type,
    },
}

/// One static (Gate 3, `docs/spec.md`, *Statics*): storage its module's unit defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDef {
    pub module: ModuleId,
    pub name: String,
    pub value: StaticValue,
    /// Where it is declared: its name.
    pub at: Span,
    /// The section `@section` places it in (Gate 3), if any.
    pub section: Option<String>,
}

/// What a static starts as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticValue {
    /// A number or a `Bool`, read-only: every use is this value, written in its place.
    Value(crate::numeric::ConstValue),
    /// An `Atomic`'s cell, starting at this `Int`.
    Cell(i64),
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
    /// Keyed by the span of the variable mention, which is written once per use — and by
    /// the name a `let` or a pattern field introduces, which resolves to its own binding.
    vars: HashMap<Span, LocalRef>,
    /// Keyed by the span of the **field name** in a projection, a field assignment or a
    /// record construction's initialiser, which is written once per access.
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
    /// What each `for` loop iterates, by the span of the loop's name (R1a): the sequence's
    /// type, which the checker settled and the provenance analysis reads to name its cell.
    for_seqs: HashMap<Span, Type>,
    /// Keyed by the span of the **`?`** of a propagation, which is written once per one.
    propagations: HashMap<Span, Propagation>,
    /// Keyed by the span of a **user-defined type's name** wherever one is written: in a
    /// parameter, a return, a field or a payload field's type, a type argument at any
    /// depth, a construction's head and the enum before the `.` of a variant or a pattern.
    /// N18: the checker decided which record or enum each of these names, and until then
    /// kept the answer only as a `Type`, so a record's uses were its constructions and an
    /// enum had none. A built-in and a type parameter are not recorded: neither is a
    /// definition a program wrote.
    type_names: HashMap<Span, TypeNode>,
    /// The checker's scopes and environments, as it built them (N19): what may be written
    /// where, for a tool to enumerate. Nothing resolves through this.
    scopes: crate::scope::ScopeTrace,
    /// Every call the checker checked against a resolved callee, by the span of the
    /// callee's name (N20). For tooling; nothing about compilation reads it.
    checked_calls: HashMap<Span, CheckedCall>,
    /// The type a member name that did not resolve was looked for on (N21), by the span of
    /// that name: a projection's field that its record lacks, a variant its enum lacks, and
    /// the name after `E.` where `E` is not a value but names an enum. Written only where a
    /// lookup failed, so a program that checks holds none — the names that did resolve are
    /// already in `fields` and `variants`, whose identities say which record or enum.
    unresolved_members: HashMap<Span, Type>,
    /// Each closure expression's definition, by the expression's span (N50).
    closures: HashMap<Span, FnId>,
    /// What the checker decided about each numeric literal whose type is not the default, and
    /// each conversion, by the literal's or the callee's span (Gate 2).
    numeric: HashMap<Span, NumericFact>,
    /// Each use of a constant, by the name's span, with the value the checker computed
    /// (Gate 2): what Core IR writes as a literal.
    const_uses: HashMap<Span, crate::numeric::ConstValue>,
    /// Each use of a static `Atomic`, by the name's span (Gate 3): its module and name, which
    /// is the one cell every use reaches.
    static_cells: HashMap<Span, (ModuleId, String)>,
    /// Every static of the compilation, in the order its module declared it (Gate 3): what the
    /// module's own unit defines.
    statics: Vec<StaticDef>,
    /// Every constant's declaration, by its name's span (Gate 2), for the reference index.
    const_decls: Vec<Span>,
    /// Every name of a constant — in a body, another constant's value or an array's length —
    /// with the declaration it names, by the name's span.
    const_refs: HashMap<Span, Span>,
    /// Each array type where it was first written (Gate 2), for the stage that holds every
    /// array to its element and size rules once every definition is known.
    arrays_written: Vec<(Type, Span)>,
    /// How many of [`Self::arrays_written`] a stage has held to the rules already.
    arrays_validated: usize,
    /// Traits, impls, bounds and calls through a bound (N78).
    traits: crate::traits::Traits,
    /// Each named function written as a value, by the name's span (N50).
    fn_values: HashMap<Span, FnValueUse>,
    /// What each call to a function with an effect parameter bound it to, by the callee's
    /// span (N51). An effect parameter in a binding is the caller's own.
    effect_bindings: HashMap<Span, crate::EffectSet>,
    /// Every sink as provenance solved it, in module and body order (N52).
    sinks: Vec<crate::SinkReport>,
    /// Each function's `result`, the slot its `ensures` clauses read (N87).
    contract_results: HashMap<FnId, u32>,
    /// Each call whose callee's preconditions the checker proved, by the callee name's span,
    /// with the witness: each clause as evaluated (N87).
    proved_calls: HashMap<Span, String>,
    /// What each written type name the checker resolved to something **no program
    /// declared** is (N23), by the span of that name: a built-in type (`Int`, `Str`, the
    /// `Vec` of `Vec[Int]`) or a type parameter — and, for a type parameter, the span its
    /// `[T]` declares it at. The names that resolved to a record or an enum are in
    /// `type_names` and are uses in the reference index; these are not, because neither a
    /// built-in nor a type parameter is an entity a program declares.
    written_types: HashMap<Span, WrittenType>,
    /// Whether this is a completion probe's compilation (N26) — never an ordinary one.
    probing: bool,
    /// For a completion probe only (N26): the callee name of the call the probe's hole is
    /// in, so that the checker, finding no function by that name, can say which record the
    /// module's types name by it. `None` in every ordinary compilation.
    completion_call: Option<Span>,
}

/// A written type name that names no declared entity, as the checker resolved it (N23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WrittenType {
    /// A built-in type, as the type the checker resolved the name to: `Int` is
    /// `Type::Int`, and the `Vec` of `Vec[Int]` is the vector type it applies to.
    Builtin(Type),
    /// A use of a type parameter.
    Param(TypeParam),
    /// A type parameter's declaration, in a `[…]` list.
    ParamDeclaration(TypeParam),
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

    /// Every trait and impl of the compilation (N78).
    #[must_use]
    pub fn traits(&self) -> &crate::traits::Traits {
        &self.traits
    }

    /// The trait tables, for the checker that is building them.
    pub fn traits_mut(&mut self) -> &mut crate::traits::Traits {
        &mut self.traits
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

    /// What the `?` at `span` takes apart and returns.
    #[must_use]
    pub fn propagation(&self, span: Span) -> Option<Propagation> {
        self.propagations.get(&span).copied()
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

    /// The call whose callee was written at `span`, as the checker checked it (N20).
    #[must_use]
    pub fn checked_call(&self, span: Span) -> Option<&CheckedCall> {
        self.checked_calls.get(&span)
    }

    /// Every checked call, by its callee's span, in no particular order.
    pub fn checked_calls(&self) -> impl Iterator<Item = (Span, &CheckedCall)> {
        self.checked_calls.iter().map(|(s, c)| (*s, c))
    }

    /// Record the call whose callee was written at `span`.
    pub fn record_checked_call(&mut self, span: Span, call: CheckedCall) {
        self.checked_calls.insert(span, call);
    }

    /// What the member name written at `span`, which did not resolve, was looked for on:
    /// a record, or an enum (N21).
    #[must_use]
    pub fn looked_up_on(&self, span: Span) -> Option<Type> {
        self.unresolved_members.get(&span).copied()
    }

    /// How many unresolved member names are recorded.
    #[must_use]
    pub fn unresolved_member_count(&self) -> usize {
        self.unresolved_members.len()
    }

    /// Record that the member name at `span` was looked for on `on` and not found.
    /// The callee name of a completion probe's hole (N26), or `None` in every ordinary
    /// compilation.
    #[must_use]
    pub const fn completion_call(&self) -> Option<Span> {
        self.completion_call
    }

    /// Whether this is a completion probe's compilation (N26).
    #[must_use]
    pub const fn probing(&self) -> bool {
        self.probing
    }

    /// Mark this a completion probe's compilation, and name the callee of its hole if the
    /// hole is in a call, before checking (N26).
    pub fn set_completion_probe(&mut self, callee: Option<Span>) {
        self.probing = true;
        self.completion_call = callee;
    }

    pub fn record_unresolved_member(&mut self, span: Span, on: Type) {
        self.unresolved_members.insert(span, on);
    }

    /// What the type name written at `span` is, when it names a built-in or a type
    /// parameter, or declares a type parameter (N23).
    #[must_use]
    pub fn written_type(&self, span: Span) -> Option<WrittenType> {
        self.written_types.get(&span).copied()
    }

    /// How many written built-in and type-parameter names are recorded (N23).
    #[must_use]
    pub fn written_type_count(&self) -> usize {
        self.written_types.len()
    }

    /// Every written built-in and type-parameter name, by span, in no particular order.
    /// For audits: a tool asks [`Self::written_type`] of one span.
    pub fn written_types(&self) -> impl Iterator<Item = (Span, WrittenType)> + '_ {
        self.written_types.iter().map(|(s, w)| (*s, *w))
    }

    /// Record what the type name written at `span` is. The same span may be resolved more
    /// than once — a signature at declaration and again with its body — and always to the
    /// same answer.
    pub fn record_written_type(&mut self, span: Span, what: WrittenType) {
        self.written_types.insert(span, what);
    }

    /// Forget every recorded use or declaration of a parameter of `owner`: a definition the
    /// checker refused, whose id will be another's (N23).
    pub fn forget_type_params_of(&mut self, owner: crate::types::ParamOwner) {
        self.written_types.retain(|_, w| match w {
            WrittenType::Param(p) | WrittenType::ParamDeclaration(p) => p.owner != owner,
            WrittenType::Builtin(_) => true,
        });
    }

    /// What the checker recorded about its scopes and environments.
    #[must_use]
    pub fn scopes(&self) -> &crate::scope::ScopeTrace {
        &self.scopes
    }

    /// The scope trace, for the checker that is recording it.
    pub fn scopes_mut(&mut self) -> &mut crate::scope::ScopeTrace {
        &mut self.scopes
    }

    /// Which record or enum the type name written at `span` names.
    #[must_use]
    pub fn type_name(&self, span: Span) -> Option<TypeNode> {
        self.type_names.get(&span).copied()
    }

    /// Record that the type name written at `span` names `to`. The same span may be
    /// resolved more than once — a signature is resolved at declaration and again when its
    /// body is checked — and always to the same answer.
    pub fn record_type_name(&mut self, span: Span, to: TypeNode) {
        self.type_names.insert(span, to);
    }

    /// Which slot the variable mentioned at `span` refers to.
    #[must_use]
    pub fn var(&self, span: Span) -> Option<u32> {
        self.vars.get(&span).map(|l| l.slot)
    }

    /// Which binding — which function's slot — the variable mentioned at `span` refers
    /// to.
    #[must_use]
    pub fn local(&self, span: Span) -> Option<LocalRef> {
        self.vars.get(&span).copied()
    }

    /// Every use this resolution recorded, with what it was resolved to, in a fixed order
    /// of kinds — calls, named functions written as values (N50, indexed since R1-C1),
    /// variables, constructions, variants, fields, payload fields and (N18) type names — and
    /// in no particular order within one. For [`crate::references`], which is the only
    /// consumer that wants them all rather than one at a time.
    ///
    /// A call to a built-in is not here: it refers to nothing a program declared.
    pub(crate) fn each_use(&self, mut f: impl FnMut(Span, crate::references::ReferenceTarget)) {
        use crate::references::ReferenceTarget as T;
        for (span, callee) in &self.calls {
            if let Callee::Function(id) = callee {
                f(*span, T::Function(*id));
            }
        }
        // `apply_twice(double, 5)`: the name of a function written as a value refers to it as
        // much as a call does. The checker recorded which function, by the name's span.
        for (span, value) in &self.fn_values {
            f(*span, T::Function(value.function));
        }
        for (span, local) in &self.vars {
            f(*span, T::Local(*local));
        }
        for (span, id) in &self.constructions {
            f(*span, T::Record(*id));
        }
        for (span, v) in &self.variants {
            f(*span, T::Variant(*v));
        }
        for (span, field) in &self.fields {
            f(*span, T::Field(*field));
        }
        for (span, p) in &self.payloads {
            f(*span, T::PayloadField(*p));
        }
        for (span, node) in &self.type_names {
            f(
                *span,
                match node {
                    TypeNode::Record(id) => T::Record(*id),
                    TypeNode::Enum(id) => T::Enum(*id),
                },
            );
        }
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
            self.by_module.resize_with(m + 1, HashMap::default);
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

    /// Declare a closure (N50): a definition no module namespace holds, so it is never found
    /// by name and never collides with one.
    pub fn declare_closure(&mut self, def: FnDef) -> FnId {
        let id = FnId::from_index(self.functions.len());
        if let Some(info) = &def.closure {
            self.closures.insert(info.at, id);
        }
        self.functions.push(def);
        id
    }

    /// Record a capture a closure's body just made (N50).
    pub fn record_capture(&mut self, closure: FnId, capture: Capture) {
        if let Some(info) = self
            .functions
            .get_mut(closure.index())
            .and_then(|f| f.closure.as_mut())
        {
            info.captures.push(capture);
        }
    }

    /// The binding a local is: itself, or — for a closure's copy of a captured binding — the
    /// binding it copies, through every closure between (N50). What a reader means by a
    /// name, so references and rename see one binding where the backend sees several slots.
    #[must_use]
    pub fn binding_of(&self, mut l: LocalRef) -> LocalRef {
        for _ in 0..=self.functions.len() {
            let Some(info) = self
                .functions
                .get(l.function.index())
                .and_then(|f| f.closure.as_ref())
            else {
                break;
            };
            let Some(c) = info.captures.iter().find(|c| c.inner == l.slot) else {
                break;
            };
            l = LocalRef {
                function: info.enclosing,
                slot: c.outer,
            };
        }
        l
    }

    /// The closure an expression at `span` made (N50).
    #[must_use]
    pub fn closure_at(&self, span: Span) -> Option<FnId> {
        self.closures.get(&span).copied()
    }

    /// Record the slot a function's `ensures` clauses read as `result` (N87).
    pub fn record_contract_result(&mut self, function: FnId, slot: u32) {
        self.contract_results.insert(function, slot);
    }

    /// The slot a function's `ensures` clauses read as `result`, if it has any (N87).
    #[must_use]
    pub fn contract_result(&self, function: FnId) -> Option<u32> {
        self.contract_results.get(&function).copied()
    }

    /// Record a call whose callee's preconditions were proved, with the witness (N87).
    pub fn record_proved_call(&mut self, at: Span, witness: String) {
        self.proved_calls.insert(at, witness);
    }

    /// The witness for a proved call, by its callee name's span (N87).
    #[must_use]
    pub fn proved_call(&self, at: Span) -> Option<&str> {
        self.proved_calls.get(&at).map(String::as_str)
    }

    /// Record a named function written as a value (N50).
    pub fn record_fn_value(&mut self, value: FnValueUse) {
        self.fn_values.insert(value.at, value);
    }

    /// Record a solved sink (N52).
    pub fn record_sink(&mut self, report: crate::SinkReport) {
        self.sinks.push(report);
    }

    /// Every solved sink, in the order provenance reported them (N52).
    #[must_use]
    pub fn sinks(&self) -> &[crate::SinkReport] {
        &self.sinks
    }

    /// Record what a call bound its callee's effect parameter to (N51).
    pub fn record_effect_binding(&mut self, at: Span, binding: crate::EffectSet) {
        self.effect_bindings.insert(at, binding);
    }

    /// What the call at `at` bound its callee's effect parameter to: the empty set where
    /// nothing did (N51).
    #[must_use]
    pub fn effect_binding(&self, at: Span) -> crate::EffectSet {
        self.effect_bindings
            .get(&at)
            .copied()
            .unwrap_or(crate::EffectSet::EMPTY)
    }

    /// The named function a value at `span` is (N50).
    #[must_use]
    pub fn fn_value(&self, span: Span) -> Option<FnValueUse> {
        self.fn_values.get(&span).copied()
    }

    /// Every named function written as a value, in source order (N50).
    #[must_use]
    pub fn fn_values(&self) -> Vec<FnValueUse> {
        let mut out: Vec<FnValueUse> = self.fn_values.values().copied().collect();
        out.sort_by_key(|u| (u.at.file, u.at.start));
        out
    }

    /// What the checker decided about the literal or conversion at `span` (Gate 2).
    #[must_use]
    pub fn numeric(&self, span: Span) -> Option<NumericFact> {
        self.numeric.get(&span).copied()
    }

    pub fn record_numeric(&mut self, span: Span, fact: NumericFact) {
        self.numeric.insert(span, fact);
    }

    /// The constant a name written at `span` is, with its value (Gate 2).
    #[must_use]
    pub fn const_use(&self, span: Span) -> Option<&crate::numeric::ConstValue> {
        self.const_uses.get(&span)
    }

    pub fn record_const_use(&mut self, span: Span, value: crate::numeric::ConstValue) {
        self.const_uses.insert(span, value);
    }

    /// The static `Atomic` a name written at `span` is, by its module and name (Gate 3).
    #[must_use]
    pub fn static_cell(&self, span: Span) -> Option<&(ModuleId, String)> {
        self.static_cells.get(&span)
    }

    pub fn record_static_cell(&mut self, span: Span, module: ModuleId, name: String) {
        self.static_cells.insert(span, (module, name));
    }

    /// Every static of the compilation (Gate 3).
    #[must_use]
    pub fn statics(&self) -> &[StaticDef] {
        &self.statics
    }

    pub fn record_static(&mut self, s: StaticDef) {
        if !self
            .statics
            .iter()
            .any(|d| d.module == s.module && d.name == s.name)
        {
            self.statics.push(s);
        }
    }

    /// Every constant declared, by its name's span (Gate 2).
    #[must_use]
    pub fn const_decls(&self) -> &[Span] {
        &self.const_decls
    }

    /// Every name of a constant, with the declaration it names (Gate 2).
    #[must_use]
    pub const fn const_refs(&self) -> &HashMap<Span, Span> {
        &self.const_refs
    }

    pub fn record_const_decl(&mut self, at: Span) {
        if !self.const_decls.contains(&at) {
            self.const_decls.push(at);
        }
    }

    pub fn record_const_ref(&mut self, at: Span, decl: Span) {
        self.const_refs.insert(at, decl);
    }

    /// Every array type written, with where it was first (Gate 2).
    #[must_use]
    pub fn arrays_written(&self) -> &[(Type, Span)] {
        &self.arrays_written
    }

    /// The array types written since the last call, each once: a stage that resolves types
    /// holds what it wrote to the rules, and none twice.
    pub fn take_arrays_to_validate(&mut self) -> Vec<(Type, Span)> {
        let new = self
            .arrays_written
            .get(self.arrays_validated..)
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        self.arrays_validated = self.arrays_written.len();
        new
    }

    pub fn record_array_written(&mut self, t: Type, at: Span) {
        if !self.arrays_written.iter().any(|(u, _)| *u == t) {
            self.arrays_written.push((t, at));
        }
    }

    pub fn record_call(&mut self, span: Span, callee: Callee) {
        self.calls.insert(span, callee);
    }

    pub fn record_var(&mut self, span: Span, to: LocalRef) {
        self.vars.insert(span, to);
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

    pub fn record_propagation(&mut self, span: Span, to: Propagation) {
        self.propagations.insert(span, to);
    }

    pub fn record_payload(&mut self, span: Span, to: PayloadRef) {
        self.payloads.insert(span, to);
    }

    /// Record the type arguments a generic call site was checked with.
    pub fn record_type_args(&mut self, span: Span, args: Vec<Type>) {
        self.type_args.insert(span, args);
    }

    /// Record what the `for` loop whose name is at `span` iterates (R1a).
    pub fn record_for_seq(&mut self, span: Span, seq: Type) {
        self.for_seqs.insert(span, seq);
    }

    /// What the `for` loop whose name is at `span` iterates — absent for a range (R1a).
    #[must_use]
    pub fn for_seq(&self, span: Span) -> Option<Type> {
        self.for_seqs.get(&span).copied()
    }

    /// The type arguments a generic call site was checked with, if it had any.
    #[must_use]
    pub fn type_args(&self, span: Span) -> Option<&[Type]> {
        self.type_args.get(&span).map(Vec::as_slice)
    }

    /// Record a call a function's body makes.
    /// Record what a function's body requires, once its module's effects are settled
    /// (N36): the set and, per effect, the first thing that requires it.
    pub fn record_effects(
        &mut self,
        id: FnId,
        inferred: crate::EffectSet,
        reasons: Vec<crate::EffectReason>,
    ) {
        if let Some(f) = self.functions.get_mut(id.index()) {
            f.inferred_effects = Some(inferred);
            f.effect_reasons = reasons;
        }
    }

    /// Record a function's solved provenance (N38).
    pub fn record_provenance(&mut self, id: FnId, summary: crate::Summary) {
        if let Some(f) = self.functions.get_mut(id.index()) {
            f.provenance = Some(summary);
        }
    }

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
