//! The representation itself.
//!
//! Locals are slots, not names: resolution finishes in [`crate::lower`], so the emitter
//! never sees an identifier and cannot accidentally reintroduce a scoping rule. Every
//! operation that can fail carries the span the failure should be reported at, because a
//! runtime error with no position is the class of error this project exists to stop
//! producing — and the generated program has no syntax tree to consult.
//!
//! # Types are carried, not inferred
//!
//! Every expression can state its own type through [`Expr::ty`]. The emitter needs the
//! type of an `if` to write its `phi`, and the type of an `==` to choose how to compare —
//! and re-deriving either from surrounding context is how a backend acquires a second,
//! subtly different type system. So `Load`, `If` and `Compare` record what the checker
//! already established, and nothing downstream guesses.
//!
//! # Why arithmetic and comparison are separate operators
//!
//! The syntax tree has one `BinOp` covering both. Splitting them here is deliberate: they
//! have different operand rules (`+` is `Int` only; `==` admits any single type), different
//! result types, and different failure modes (`+` can overflow, `<` cannot). One enum
//! would let a `match` arm written for arithmetic silently start accepting comparisons.

use nazm_sema::ModuleId;

use crate::symbol::{NativeSymbol, NativeTypeName};

/// The value types this backend can represent. Not the language's — `Str` exists and is
/// refused by name in [`crate::lower`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ty {
    Int,
    /// A fixed-width integer or a float (Gate 2): a scalar of its kind's class.
    Num(nazm_sema::NumKind),
    Bool,
    /// An immutable byte sequence, as `{ pointer, byte length, owner }` by value.
    ///
    /// Not NUL-terminated: `docs/spec.md` makes an embedded zero an ordinary byte, so
    /// the length is the length. The pointer is always valid to read `length` bytes
    /// from, and for an empty string it still points at real storage — which is what
    /// lets `memcmp` be called without a special case.
    ///
    /// # The third field, and why it is not optional
    ///
    /// `owner` names **the allocation whose lifetime keeps `pointer` valid**, or null
    /// when nothing has to. It was added in N8 for one reason: a `Str` may point at a
    /// literal, at a process argument, at a heap buffer, or at the *interior* of any of
    /// those — `str_slice` is a `getelementptr` and produces no copy — and
    /// `{ pointer, length }` cannot tell them apart. Any rule for reclaiming a string
    /// written over two fields would have had to guess from the pointer, which is the
    /// class of heuristic `docs/architecture.md` §7.8 rules out.
    ///
    /// It is not language-visible. No Nazm expression can read it, and `docs/spec.md`
    /// still specifies a `Str` as its bytes.
    Str,
    /// A growable sequence of `Int`: a **pointer to a header**, `{ len, cap, data }`.
    ///
    /// The handle is the header's address and never changes, which is what gives these
    /// reference semantics: growth reallocates `data` and rewrites the header in place,
    /// so every holder of the handle sees it. See `docs/spec.md`, *Sequences are shared,
    /// mutable, and never freed*.
    Ints,
    /// The same, with 24-byte `{ ptr, i64, ptr }` elements, each owning its backing.
    Strs,
    /// A bounded queue of `Int`, as a **pointer to a runtime header**.
    ///
    /// Reference semantics like a sequence, and for a stronger reason: a channel exists
    /// to be shared between tasks, and every one of its operations is a
    /// synchronisation point. See `docs/spec.md`, *Structured concurrency*.
    Chan,
    /// A user-defined record: an **LLVM aggregate, by value**.
    ///
    /// Not a pointer, and that is the semantic decision rather than a representation
    /// choice: `docs/spec.md` gives a record no object identity, so two bindings of one
    /// record are two values and nothing may be visible through both. A pointer would have
    /// made every assignment an alias.
    ///
    /// What it *contains* is another matter: a `Str` field carries an owner, a sequence
    /// field carries a handle, and copying the aggregate copies those — which is why a
    /// record copy retains field by field rather than not at all. The id indexes the
    /// unit's [`RecordLayout`] table.
    Record(RecordTy),
    /// A user-defined enum: an **LLVM aggregate, by value**, of a discriminant and one
    /// slot per variant.
    ///
    /// The value semantics are a record's and for the same reason. What differs is that
    /// only one of the payload slots means anything at a time, and which one is the first
    /// field's value — see [`EnumLayout`] for the representation and what it costs.
    Enum(EnumTy),
    /// A `Vec[T]` handle: a pointer to the sequence header every sequence has, whose
    /// elements are `T`'s own representation laid out contiguously. N11. Which `T` is in
    /// the unit's [`VecLayout`] table, by this id.
    Vec(VecTy),
    /// A function value (N50): a pointer to a closure object, `{ count, code, release,
    /// captures… }`, by its index in the program's function types. Null when it owns nothing.
    Fn(FnTy),
    /// A `Chan[T]` (N53): a pointer to a generic channel, by its index in the program's
    /// channel types. Null when it owns nothing.
    ChanOf(ChanTy),
    /// `[T; N]` (Gate 2): held in memory, `N` elements of plain data, owning nothing.
    Array(ArrayTy),
}

/// An array type, by its index in the program's array types (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrayTy(pub u32);

/// An array type's element and length (Gate 2): `len` elements of `elem`'s layout, contiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayLayout {
    pub id: ArrayTy,
    pub elem: Ty,
    pub len: u32,
}

/// A generic channel type, by its index in the program's channel types (N53).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChanTy(pub u32);

/// A function type, by its index in the program's function types (N50).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnTy(pub u32);

/// An enum type of this compilation, by declaration order. Session-local, like
/// [`RecordTy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumTy(pub u32);

/// A `Vec` type of this compilation, by its session-local interned id.
///
/// The same rule as [`RecordTy`]: an index shared with the checker's table rather than a
/// second numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VecTy(pub u32);

impl VecTy {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a unit needs to know about one concrete `Vec[T]`: its element's type, and the name
/// its generated release helper is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VecLayout {
    pub id: VecTy,
    /// Derived from the element's canonical type key, so two units that release one
    /// `Vec[Point]` generate one helper name — each its own `internal` copy, which is what
    /// keeps a unit independent of another's text.
    pub name: String,
    pub elem: Ty,
    /// The element's source spelling, for a comment.
    pub source: String,
}

impl EnumTy {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One variant's payload, as a type of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantLayout {
    /// The source name, for comments in `--emit-ir` and for the helper's symbol. Never
    /// resolved against.
    pub source: String,
    /// What LLVM calls this variant's payload struct, derived from the enum's durable
    /// identity and the variant's name.
    pub name: NativeTypeName,
    /// The payload field types **in layout order**, canonical by field name.
    pub fields: Vec<Ty>,
    /// For each payload field in **declaration** order, its index in [`Self::fields`].
    pub of_declaration: Vec<u32>,
}

/// How one enum is laid out, and what each variant's payload owns.
///
/// # The representation, and what it costs
///
/// `{ i64, %V0, %V1, … }` — a discriminant followed by **one slot per variant**, in
/// canonical variant-name order, each slot that variant's payload struct. A value is built
/// from `zeroinitializer` and the active slot is filled, so every byte is defined and
/// copying the aggregate is defined; the inactive slots hold zeros, which no operation
/// reads.
///
/// This is larger than the union it stands for: an enum of three `Str` payloads is
/// 8 + 24 × 3 bytes where an ideal tagged union is 8 + 24. That is a real cost and
/// `docs/performance.md` measures it. It was chosen over a `[N x i64]` payload area with
/// typed loads and stores through it because that needs the compiler to compute each
/// variant's size exactly right — an arithmetic mistake there is a buffer overflow rather
/// than a compile error — and because this form needs no memory at all: a variant is built
/// with `insertvalue` and a payload read with `extractvalue`, entirely in registers.
/// `docs/spec.md` promises nothing about an enum's size or layout, so the representation
/// can change without the language moving.
///
/// # Why the order is by name
///
/// The same rule the field order follows: `docs/spec.md` makes variant declaration order
/// non-semantic, so the discriminant is derived from the **names**. Moving two variant
/// declarations therefore changes no byte of the object, where taking the parser's order
/// would have created an ABI difference from an edit with no meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumLayout {
    pub id: EnumTy,
    /// What LLVM calls this type, derived from the enum's durable identity.
    pub name: NativeTypeName,
    /// The source name, for comments in `--emit-ir`. Never resolved against.
    pub source: String,
    /// The variants **in canonical order**, which is tag order: the position in this list
    /// **is** the discriminant, and the slot in the aggregate is one past it.
    pub variants: Vec<VariantLayout>,
    /// For each variant in **declaration** order, its discriminant.
    ///
    /// The enum's half of the layout boundary. A consumer holding a
    /// [`VariantRef`](nazm_sema::VariantRef) — which names a declaration index, because
    /// that is what a variant's semantic identity is spelled as in one session — comes
    /// through here and gets a number.
    pub of_declaration: Vec<u32>,
}

/// A record type of this compilation, by declaration order.
///
/// Session-local, like every id in `nazm-sema`. What crosses a compilation boundary is
/// the [`NativeTypeName`] derived from the record's durable identity, which is what makes
/// two independently generated objects agree about a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordTy(pub u32);

impl RecordTy {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// How one record is laid out, and what its fields own.
///
/// # The layout boundary, on this side of it
///
/// The front end knows a record identity and a field identity — `docs/architecture.md`
/// requires field access to stay symbolic until here — and this is where that becomes a
/// position. [`Self::of_declaration`] is the map, and it is the only place a field's index
/// is decided.
///
/// # Why the order is by name
///
/// `docs/spec.md` makes declaration order non-semantic: moving two field declarations is
/// not a change to the type. Laying fields out in **canonical field-name order** makes
/// that true of the object file as well — an edit the language says means nothing also
/// changes no byte of the emitted code. Laying them out in declaration order would have
/// created an ABI difference from an edit with no meaning, which is the worst kind: the
/// compiler would be right that nothing changed and the linker would disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordLayout {
    pub id: RecordTy,
    /// What LLVM calls this type, derived from the record's durable identity.
    pub name: NativeTypeName,
    /// The source name, for comments in `--emit-ir`. Never resolved against.
    pub source: String,
    /// The field types **in layout order**, which is the order of the LLVM struct.
    pub fields: Vec<Ty>,
    /// For each field in **declaration** order, its index in [`Self::fields`].
    ///
    /// The whole of the translation. A consumer holding a
    /// [`FieldRef`](nazm_sema::FieldRef) — which names a declaration index, because that
    /// is what a field's semantic identity is spelled as in one session — comes through
    /// here and gets a position.
    pub of_declaration: Vec<u32>,
}

/// What a value of a given type owns, and therefore what has to happen when a binding
/// takes one on or lets one go.
///
/// One classification for the whole backend, rather than a test of the type at each place
/// cleanup is emitted. That is the shape `docs/architecture.md` §7.9 asks for: adding a
/// type to the language means answering this enum once, and every exit edge, every
/// binding and every container follows from the answer. It is deliberately **not** a
/// destructor system — Nazm has no user-defined destruction, and each variant names a
/// runtime operation that already exists rather than a hook a program could install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owns {
    /// A sequence header holding `Int`s, and its data buffer.
    Ints,
    /// A sequence header holding `Str`s. Releasing it releases **every element's
    /// backing** first: a container that freed only its array of descriptors would turn
    /// a leaked string into a leaked string inside a sequence.
    Strs,
    /// A string's backing allocation, named by the value's third field, which may be
    /// null — a literal and a process argument own nothing, and releasing null is what
    /// says so.
    Backing,
    /// A channel header, with its queue, its mutex and its condition variable.
    Channel,
    /// Whatever its fields own, **recursively**.
    ///
    /// The variant that makes this enum a classification rather than a list: a record
    /// does not own a thing of its own — it has no header, no count and no allocation —
    /// and what happens when one is taken on or let go is entirely its fields' rules,
    /// applied in turn. Adding a record type to a program therefore adds no case here,
    /// which is the property `docs/architecture.md` §7.9 asked for before records existed.
    Record(RecordTy),
    /// Whatever the **active variant's** fields own, recursively.
    ///
    /// The second composition, and the one N10 is a test of. A record's rule is a walk
    /// over its fields; an enum's is a branch on its discriminant and then a walk over one
    /// variant's. Neither is a case in a consumer: both are a helper generated once per
    /// type, so adding an enum to a program adds no arm here either.
    Enum(EnumTy),
    /// A `Vec` header, its data buffer, and **every element's** ownership, by the
    /// element's law. N11. Releasing the last reference releases each element and then
    /// the storage, through a helper generated once per element type.
    Vec(VecTy),
    /// A reference to a closure object, which releases its captures on the last one (N50).
    Closure,
    /// A reference to a generic channel, which releases what it still holds on the last one
    /// (N53).
    GenericChannel,
}

impl Ty {
    /// What a value of this type owns, if anything.
    ///
    /// `Int` and `Bool` own nothing and never will. Everything else does, since N8:
    /// `docs/spec.md`'s memory constitution reclaims storage when the last reference to
    /// it dies, and this is the list of things that have a reference to count.
    #[must_use]
    pub fn owns(self) -> Option<Owns> {
        match self {
            Self::Int | Self::Bool | Self::Num(_) | Self::Array(_) => None,
            Self::Str => Some(Owns::Backing),
            Self::Ints => Some(Owns::Ints),
            Self::Strs => Some(Owns::Strs),
            Self::Chan => Some(Owns::Channel),
            // Unconditional, even for a record whose every field is an `Int`: answering
            // truthfully would need the layout table, which this does not have. The
            // emitter asks the table before it registers a frame slot, so a record that
            // owns nothing still costs nothing — it simply costs nothing one level later.
            Self::Record(id) => Some(Owns::Record(id)),
            // Unconditional for the reason a record's is, and one step stronger: the
            // answer depends on every variant's payload, which this does not have.
            Self::Enum(id) => Some(Owns::Enum(id)),
            // A handle's storage is its own, whatever it holds.
            Self::Vec(id) => Some(Owns::Vec(id)),
            Self::Fn(_) => Some(Owns::Closure),
            Self::ChanOf(_) => Some(Owns::GenericChannel),
        }
    }

    /// Whether a value of this type is a **handle** to storage with a reference count.
    ///
    /// `docs/spec.md`'s memory constitution calls these *owned mutable handles*: two
    /// bindings on one are two references to one storage, and a mutation through either
    /// is visible through both. Narrower than [`Self::owns`] on purpose — a `Str` owns a
    /// backing and is still a value, and a `Chan` is runtime-managed — and used only
    /// where *aliasing* is what matters rather than reclamation.
    #[must_use]
    pub fn is_handle(self) -> bool {
        matches!(self, Self::Ints | Self::Strs | Self::Vec(_))
    }

    /// Bytes one element of this sequence occupies, for indexing and growth.
    #[must_use]
    pub fn element_size(self) -> Option<u32> {
        match self {
            Self::Ints => Some(8),
            // `{ ptr, i64, ptr }`: N8's `Str`, which is one pointer wider than the
            // `{ ptr, i64 }` this was written for.
            Self::Strs => Some(24),
            _ => None,
        }
    }

    /// The LLVM type of one element.
    #[must_use]
    pub fn element(self) -> Option<Self> {
        match self {
            Self::Ints => Some(Self::Int),
            Self::Strs => Some(Self::Str),
            _ => None,
        }
    }
}

impl Ty {
    /// The LLVM type name of a **built-in** type.
    ///
    /// Not total, and deliberately named so: a record's spelling is its
    /// [`NativeTypeName`], which lives in the unit's layout table and is not static.
    /// Every site that can meet a record goes through the emitter's own `llvm_type`,
    /// and this arm exists so that one which does not produces text LLVM rejects rather
    /// than text it silently accepts.
    #[must_use]
    pub fn builtin_llvm(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Num(k) => match k {
                nazm_sema::NumKind::I8 | nazm_sema::NumKind::U8 => "i8",
                nazm_sema::NumKind::I16 | nazm_sema::NumKind::U16 => "i16",
                nazm_sema::NumKind::I32 | nazm_sema::NumKind::U32 => "i32",
                nazm_sema::NumKind::I64 | nazm_sema::NumKind::U64 => "i64",
                nazm_sema::NumKind::F32 => "float",
                nazm_sema::NumKind::F64 => "double",
            },
            Self::Bool => "i1",
            Self::Str => "{ ptr, i64, ptr }",
            Self::Ints | Self::Strs | Self::Chan | Self::Vec(_) | Self::Fn(_) | Self::ChanOf(_) => {
                "ptr"
            }
            // A record's spelling is its name, which is not static. Every site goes
            // through the emitter's own `ty`, which has the table; this arm exists so
            // that a site which has not been converted is a visible wrong answer in the
            // IR rather than a silent one.
            Self::Record(_) => "<record>",
            Self::Enum(_) => "<enum>",
            Self::Array(_) => "<array>",
        }
    }

    /// A value of this type to return from a function that is unwinding.
    ///
    /// It is never read. A failure on its way out still has to leave every function it
    /// passes through by an ordinary `ret`, because the emitted code has no other way to
    /// leave one, and `ret` needs an operand of the right type. Every caller checks the
    /// failure flag before it looks at the result, so this stands in for a value that is
    /// already known not to exist.
    #[must_use]
    pub fn unwind_value(self) -> &'static str {
        match self {
            Self::Int => "0",
            Self::Num(k) if k.is_float() => "0.0",
            Self::Num(_) => "0",
            Self::Bool => "false",
            // Valid for any aggregate, and it is also the value a record slot holds
            // before anything is stored in it: every owning field null, so releasing it
            // is a no-op. That is what makes a partially built record safe to clean.
            // For an enum, `zeroinitializer` is a discriminant of 0 — a real variant, the
            // canonically first — with an all-zero payload. Releasing it walks that
            // variant's fields and finds null in each, which is a no-op in every one of
            // the runtime's release functions. So a zeroed enum slot is safe to release
            // exactly as a zeroed record slot is.
            Self::Str | Self::Record(_) | Self::Enum(_) | Self::Array(_) => "zeroinitializer",
            Self::Ints | Self::Strs | Self::Chan | Self::Vec(_) | Self::Fn(_) | Self::ChanOf(_) => {
                "null"
            }
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Num(k) => k.name(),
            Self::Bool => "Bool",
            Self::Str => "Str",
            Self::Ints => "Ints",
            Self::Strs => "Strs",
            Self::Chan => "Chan",
            Self::Record(_) => "a record",
            Self::Enum(_) => "an enum",
            Self::Vec(_) => "a `Vec`",
            Self::Fn(_) => "a function value",
            Self::ChanOf(_) => "a `Chan[T]`",
            Self::Array(_) => "an array",
        }
    }
}

impl Ty {
    /// The backend's representation of a front-end [`Type`](nazm_sema::Type).
    ///
    /// One-to-one, and written as a total match rather than a default arm so that adding a
    /// type to the language is a compile error here instead of a silent `Int`.
    ///
    /// `None` for a type parameter, which has no representation: native code exists only
    /// for concrete instances, and a parameter reaching here is a lowering that forgot its
    /// substitution — which `crate::lower` reports as the compiler bug it is.
    #[must_use]
    pub fn concrete(t: nazm_sema::Type) -> Option<Self> {
        match t {
            nazm_sema::Type::Param(_) => None,
            other => Some(Self::of(other)),
        }
    }

    /// [`Self::concrete`] for a type already known to be concrete. A parameter maps to
    /// `Int` here, and every caller that can meet one uses [`Self::concrete`] instead.
    #[must_use]
    pub fn of(t: nazm_sema::Type) -> Self {
        match t {
            // A `Box[T]` and a `List[T]` are their `Vec[T]`, which nothing writes (R1a).
            nazm_sema::Type::Vec(id) | nazm_sema::Type::Box(id) | nazm_sema::Type::List(id) => {
                Self::Vec(VecTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
            nazm_sema::Type::ChanOf(id) => {
                Self::ChanOf(ChanTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
            nazm_sema::Type::Param(_) => Self::Int,
            nazm_sema::Type::Int => Self::Int,
            nazm_sema::Type::Num(k) => Self::Num(k),
            nazm_sema::Type::Bool => Self::Bool,
            nazm_sema::Type::Str => Self::Str,
            // `Str`'s triple, `{ptr, len, owner}` (Gate 2): a `Bytes` is always a view of a
            // heap buffer it counts, which its built-ins write through; a `Text` is a `Str`
            // the checker knows is UTF-8.
            nazm_sema::Type::Bytes | nazm_sema::Type::Text => Self::Str,
            // `Str`'s triple too (Gate 3): `ptr` is the cell, `len` its 8 bytes, `owner` the
            // counted allocation holding it — or null for a static's, which nothing frees.
            nazm_sema::Type::Atomic => Self::Str,
            nazm_sema::Type::Ints => Self::Ints,
            nazm_sema::Type::Strs => Self::Strs,
            nazm_sema::Type::Chan => Self::Chan,
            // A capability is authority the checker has already verified, and nothing at
            // runtime consults it (N37): what is left of one is a machine word that is
            // always 0. A word rather than a zero-sized `{}`, so every path that stores,
            // copies or holds a value in a sequence keeps the representation it already
            // has — a zero-byte element would reach an allocation of zero bytes.
            nazm_sema::Type::Cap(_) => Self::Int,
            // A handle is a word: `generation << 32 | descriptor`, or a failed open's status
            // (Gate 2). The runtime's handle table holds what it names.
            nazm_sema::Type::OsHandle => Self::Int,
            // The id carries across unchanged: both sides index the same session's
            // records, and the backend's table is built from the checker's rather than
            // being a second one. `docs/architecture.md`'s single-resolution rule.
            // Not `const` since N9: a record carries an id, and converting one without
            // an `as` cast needs `try_from`. Saturating is the answer every other id
            // conversion gives, and a compilation with four billion record declarations
            // has other problems.
            nazm_sema::Type::Record(id) => {
                Self::Record(RecordTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
            nazm_sema::Type::Enum(id) => {
                Self::Enum(EnumTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
            nazm_sema::Type::Fn(id) => {
                Self::Fn(FnTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
            nazm_sema::Type::Array(id) => {
                Self::Array(ArrayTy(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            }
        }
    }
}

/// One Nazm module, lowered: everything that belongs in its own native artefact.
///
/// # What is in it, and what is deliberately not
///
/// The functions **this module declares**, the external definitions it refers to, and
/// nothing else. A module that imports another does not acquire its bodies here, which is
/// the whole of what "separately code-generated" means: the unit for `a.nz` is a function
/// of `a.nz`'s source and of its dependencies' *signatures*, and a change to a
/// dependency's body cannot alter a byte of it.
///
/// Since N40 a function is named by its MIR identity: the body the emitter writes is that
/// function's validated MIR, and nothing about it is decided here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// Which Nazm module this is. Session-local, and used for naming the artefact and
    /// reporting — never for a symbol, which comes from the durable identity.
    pub module: ModuleId,
    /// The name of the source file, for `source_filename`.
    pub source: String,
    /// The MIR functions this unit defines, in MIR order.
    pub functions: Vec<nazm_mir::FuncId>,
    /// Whether this is its module's own unit, rather than a generic instance's: the one that
    /// defines the module's statics (Gate 3), which every other unit imports.
    pub owns_module: bool,
    /// Definitions this unit calls and does not define, so the emitter can declare them.
    ///
    /// Sorted and deduplicated, so two builds of one module produce one text.
    pub externals: Vec<External>,
    /// The record types this unit's own code mentions, closed over field types.
    ///
    /// **Only the ones it uses.** A unit that never names a record carries none, and one
    /// that names `Point` does not acquire every other record in the program — which is
    /// what keeps N6's object cache precise: a record added elsewhere changes no byte of
    /// this unit's text, so its object stays reusable.
    pub records: Vec<RecordLayout>,
    /// The enum types this unit's own code mentions, closed over payload types, by the
    /// same rule and for the same reason as [`Self::records`].
    pub enums: Vec<EnumLayout>,
    /// The `Vec` types this unit's text mentions, closed over element types.
    pub vecs: Vec<VecLayout>,
    /// Every array type of the program (Gate 2): few, and laid out from their elements.
    pub arrays: Vec<ArrayLayout>,
}

/// A definition another unit owns, as this one needs to see it.
///
/// A symbol and a signature. **Not a body** — that is the point, and it is why a
/// dependency's implementation cannot reach this unit's object.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct External {
    pub symbol: NativeSymbol,
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Whether this is a foreign (C) function, declared with the C convention (N42).
    pub foreign: bool,
}

/// The built-ins this backend can lower.
///
/// Deliberately **not** a re-export of the front end's table: `nazm-lir` may not depend
/// on `nazm-core` (see `xtask/src/rules.rs`), and more importantly it should not. Which
/// built-ins compile natively is a coverage question with the same answer shape as the
/// rest of the subset, and a front-end built-in missing here must produce a backend
/// limitation diagnostic rather than a miscompile. A test in `nazm-cli`, which sees both
/// A built-in, as the backend sees it.
///
/// **The same type the front end resolves**, aliased rather than transcribed. Until
/// 2026-09-21 this crate declared its own 31-variant enum with its own `by_name` and
/// `name` tables, and a test in `nazm-cli` — the only crate that can see both — kept the
/// two in agreement. Two inventories agreeing because a test says so is not one
/// inventory; a built-in added to one and forgotten in the other was a test failure
/// rather than a compile error, and only because someone had written that test.
///
/// Identity, name and signature are `nazm_sema::Intrinsic`'s. What stays here is how a
/// built-in is *emitted*, which is genuinely backend-specific — see [`Lowering`].
pub use nazm_sema::Intrinsic as Builtin;

/// What lowering needs to know about a built-in, over and above its identity.
///
/// An extension trait rather than inherent methods, because the type belongs to
/// `nazm-syntax` now. The split is the point: **the front end owns what a built-in *is* —
/// its name, its identity, its signature in language types — and the backend owns what it
/// *becomes*.**
///
/// The names differ from [`Intrinsic`](nazm_sema::Intrinsic)'s deliberately.
/// `Intrinsic::params` answers in `Type` and `lowered_params` answers in [`Ty`]; had both
/// been called `params`, Rust would have silently preferred the inherent one and the
/// mismatch would have surfaced as a type error somewhere else entirely.
pub trait Lowering {
    /// The sequence type this built-in operates on, if it is a sequence operation.
    fn sequence(self) -> Option<Ty>;
    /// Parameter types, in the backend's representation.
    ///
    /// Checked against the lowered arguments the same way a call's are. The front end has
    /// already established both, so a mismatch here is a *lowering* bug rather than a
    /// program error — and a wrongly typed argument produces a module clang rejects for a
    /// reason that names none of this.
    fn lowered_params(self) -> Vec<Ty>;
    /// Return type, in the backend's representation.
    fn lowered_ret(self) -> Ty;
}

impl Lowering for Builtin {
    fn sequence(self) -> Option<Ty> {
        use nazm_sema::Shape;
        match nazm_sema::Intrinsic::params(self).first() {
            Some(Shape::Ints) => Some(Ty::Ints),
            Some(Shape::Strs) => Some(Ty::Strs),
            _ => match nazm_sema::Intrinsic::ret(self) {
                Shape::Ints => Some(Ty::Ints),
                Shape::Strs => Some(Ty::Strs),
                _ => None,
            },
        }
    }

    // A generic built-in's shapes mention `T`, which has no representation here; its
    // positions map to `Int` and are never read, because a generic built-in is lowered to
    // [`Expr::Vec`], which carries its concrete types, and never to [`Expr::Builtin`].
    fn lowered_params(self) -> Vec<Ty> {
        nazm_sema::Intrinsic::params(self)
            .iter()
            .map(|s| s.monomorphic().map_or(Ty::Int, Ty::of))
            .collect()
    }

    fn lowered_ret(self) -> Ty {
        nazm_sema::Intrinsic::ret(self)
            .monomorphic()
            .map_or(Ty::Int, Ty::of)
    }
}

/// The five arithmetic operators, each of which can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl ArithOp {
    /// The word the interpreter uses in its overflow diagnostic, so the two
    /// implementations report the same sentence for the same failure.
    #[must_use]
    pub fn overflow_noun(self) -> &'static str {
        match self {
            Self::Add => "addition",
            Self::Sub => "subtraction",
            Self::Mul => "multiplication",
            Self::Div => "division",
            // `%` never overflows: see `emit`.
            Self::Rem => "remainder",
        }
    }
}

/// The six comparisons, none of which can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl CmpOp {
    /// The LLVM `icmp` predicate.
    ///
    /// Signed on purpose. `Int` is signed 64-bit, so `-1 < 1` must hold; `ult` would
    /// compare the two's-complement bit patterns and make it false. For `Bool` only `eq`
    /// and `ne` are reachable — ordering `Bool` is refused by the checker — and those two
    /// predicates carry no signedness.
    #[must_use]
    pub fn predicate(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::NotEq => "ne",
            Self::Lt => "slt",
            Self::LtEq => "sle",
            Self::Gt => "sgt",
            Self::GtEq => "sge",
        }
    }

    /// Whether this comparison orders its operands, and so needs `Int`.
    #[must_use]
    pub fn is_ordering(self) -> bool {
        !matches!(self, Self::Eq | Self::NotEq)
    }
}
