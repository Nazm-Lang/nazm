//! The persisted shape of a module's interface: `nazm.interface/4`.
//!
//! # What may appear in it
//!
//! Durable identities, names, and a canonical spelling of each exported signature. That is
//! the whole list, and the omissions are the design:
//!
//! | Not in it | Because |
//! | --- | --- |
//! | `FileId`, `ModuleId`, `FnId` | indices into one compilation; meaningless to the run that reads this |
//! | `Span` | a position in a file, which says nothing about what a module offers — and moving a function must not change its interface |
//! | private definitions | they are not offered |
//! | bodies | the point of an interface is that a dependent needs none |
//! | absolute paths | they say where a checkout lives, and two checkouts of one project must produce identical bytes |
//! | timestamps, build ids, load order | none of them is part of what a module means |
//! | type parameter names | a parameter is its **position**; renaming `T` to `Value` means nothing, so it changes no byte |
//! | instances, native layouts, symbols | how a backend implements a generic definition is not what a module offers |
//!
//! # Determinism
//!
//! The same interface must serialise to the same bytes, in this process and in the next
//! one. Every field is a struct field or a `Vec` — **no map**, so no iteration order to
//! depend on — exports are sorted by name and dependencies by key, and `serde_json` emits
//! struct fields in declaration order. `docs/architecture.md` §7.3 says why that matters:
//! a fingerprint over non-deterministic bytes is a fingerprint that changes when nothing
//! did.

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use nazm_sema::enums::{EnumDef, VariantDef};
use nazm_sema::interface::{EnumExport, Export, RecordExport, UnitInterface, Visibility};
use nazm_sema::key::{DefKey, DefKind, ModuleKey};
use nazm_sema::module::ModuleGraph;
use nazm_sema::record::{FieldDef, RecordDef};
use nazm_sema::resolve::FnId;
use nazm_sema::types::{ParamOwner, Type, TypeParam};
use nazm_sema::udt::{TypeNode, UserTypes};
use nazm_sema::{ModuleId, MAX_TYPE_DEPTH};
use nazm_span::Span;

/// The identifier every persisted interface carries.
///
/// `name/major`, the same rule `nazm.diagnostic/1` follows: the major number changes when
/// a reader written against the previous one would be **wrong**.
///
/// # Why N9 versioned it rather than extending `/1`
///
/// `/1` wrote every type as a built-in name. A `/1` reader handed a `/2` file would meet
/// `lib/model.nz::record Point` where it expects `Int`, and — being a reader that
/// validates — would reject it. That is the good case. The bad one is the reason for the
/// bump: a `/1` reader that *did* parse a `/2` file would conclude a module exports a
/// function of a type it cannot represent, and would then answer questions about that
/// module wrongly rather than declining. Compatibility means an old consumer stays
/// **correct**, not that parsing succeeds, and there is no extension of `/1` under which
/// an old reader stays correct about a record it has no concept of.
///
/// # Why N10 versioned it again rather than extending `/2`
///
/// The same argument, one kind over: `/2` had no representation for an enum.
///
/// # Why N11 versioned it a third time
///
/// `/3` wrote a type as a **string** — a built-in's name or a definition's key — and had
/// no way to say *this is the second parameter of the definition it appears in*, nor *this
/// definition applied to these arguments*. A generic signature is not expressible in it at
/// all. So `/4` writes a type as a small tree ([`PersistentType`]) — the one change that
/// makes `Vec[Pair[Int, $0]]` representable — and records each definition's **arity**.
/// A `/3` reader handed a `/4` file would find objects where it expects strings and must
/// refuse; there is no extension under which it could stay correct about a type argument
/// it has no concept of.
///
/// # Why N14 versioned it a fourth time
///
/// A generic function may require equality of a type parameter (`fn same[T: Equality]`),
/// and which callers are valid depends on it: `same(v, v)` for a `Vec` is refused only
/// because of the requirement. `/4` recorded a function's arity and nothing about its
/// parameters, so an importer checking against a `/4` file would accept that call. `/5`
/// writes each export's required positions (`requires_equality`). A `/4` reader handed a
/// `/5` file could not honour the field, so the major moves rather than the field being
/// added silently.
///
/// # Why N36 versioned it a fifth time
///
/// A function may declare the effects it exercises (`! { io }`), and what a dependent may
/// do with it depends on that: a function of its own declaring `! {}` may call an import
/// only if the import's declared set is empty. `/5` had nowhere to say so, and what a `/5`
/// file's silence meant was "no effects were modelled" — while in `/6` an export with no
/// `effects` field is one that **declares none**, which its importers must read as every
/// effect. The same bytes would mean two things, so the major moves: `/6` writes each
/// export's declared set as names in effect-id order, and absent only when none is declared.
///
/// # Why N37 did not version it
///
/// A capability parameter — `fn save(io: IoCap, …)` — is an ordinary parameter of a
/// built-in type, and `/6` already writes a built-in type as its name: the authority an
/// export requires is in its signature, in the form every signature has. No field was
/// added and none changed meaning. An export with no `effects` field still declares none,
/// and is still every effect to its importers; that an importer with a declared set must
/// then *hold* the authority those effects need is a checker rule, which
/// `SEMANTIC_EPOCH` 9 covers, not a reading of these bytes. The one thing an older reader
/// cannot do is know `IoCap` and `SpawnCap`, and it refuses them as unknown types rather
/// than misreading them.
///
/// Unknown majors remain refused rather than reinterpreted — including a future `/8`, and
/// including `/1` to `/6`, which this build no longer reads.
///
/// # `/7` (N42)
///
/// An export may carry `foreign`: the C symbol of a `pub extern "C" fn`. An importer calls a
/// foreign function without the declaration's source, so it must know the callee is one —
/// a call to it needs a `ForeignCap` held — and which symbol it links against. The field is
/// absent for every Nazm function, but a `/6` reader would silently take a foreign export for
/// an ordinary function with no body, so the major moves.
///
/// # `/8` (N78)
///
/// An interface carries the module's exported traits, every impl it declares, and each
/// export's trait bounds. An importer's method lookup and coherence check read the first two,
/// and checking a call reads the third; a `/7` reader would take a module with impls for one
/// without, and resolve a call differently. The fields are absent where empty, but the major
/// moves.
///
/// # `/9` (N85)
///
/// A record may carry `c`: `"struct"` for an `extern "C" struct` with fields, whose fields
/// are then written **in declaration order**, which is its C layout; `"handle"` for an opaque
/// handle, whose one field is the pointer no program can name. An importer builds, reads,
/// compares and passes a record by these, so an `/8` reader — which would take a handle for
/// an ordinary record any program may build, and a C struct's fields for an order it may
/// sort — must refuse it, and the major moves.
///
/// # `/10` (N87)
///
/// An export may carry `requires`: how many preconditions it states. An importer's obligation
/// report counts each of its calls as one, and a `/9` reader would count none; absent where it
/// states none, but the major moves.
pub const SCHEMA: &str = "nazm.interface/10";

/// A type as persisted.
///
/// A tree rather than a string, since `/4`, and each node is one of four shapes that
/// cannot be confused: a JSON string is a built-in, and an object is told apart by the one
/// field it names.
///
/// | Shape | Written | Means |
/// | --- | --- | --- |
/// | built-in | `"Int"` | the built-in of that name |
/// | parameter | `{"param":0}` | the first type parameter of the definition this type appears in |
/// | vector | `{"vec":T}` | `Vec[T]` |
/// | definition | `{"def":"a.nz::record Box","args":[T]}` | that definition applied to those arguments — none for one that is not generic |
/// | channel | `{"chan":T}` | `Chan[T]` (N53); a `/7` reader refuses it as it refuses any shape it does not know |
/// | function | `{"fn":[T],"ret":R,"effects":["io"]}` | `fn(T) -> R ! { io }` (N50), its effects by name in effect-id order |
///
/// The function shape arrived in N50 without a schema move: a `/7` reader that meets one
/// cannot read it as any other shape, refuses the file, and so treats it as no entry — it is
/// never wrong, only unable, which is what an older reader of a newer file may be.
///
/// A parameter is a **position**, never a name: that is what makes renaming `T` to
/// `Value` leave every byte, and so the fingerprint, unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PersistentType {
    Builtin(String),
    Param {
        param: u32,
    },
    Vec {
        vec: Box<PersistentType>,
    },
    /// `Chan[T]` (N53), written `{"chan":T}`.
    Chan {
        chan: Box<PersistentType>,
    },
    Def {
        def: String,
        args: Vec<PersistentType>,
    },
    Fn {
        #[serde(rename = "fn")]
        params: Vec<PersistentType>,
        ret: Box<PersistentType>,
        effects: Vec<String>,
    },
}

/// One field of a published record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentField {
    /// The field's name, which **is** its identity. Never its position: `docs/spec.md`
    /// makes declaration order non-semantic, so a durable form keyed on an index would
    /// change meaning when an author moved two lines.
    pub name: String,
    /// Its type. A parameter in it is one of the owning definition's.
    pub ty: PersistentType,
}

/// One variant of a published enum.
///
/// The variant's **name** is its identity, exactly as a field's is. The numeric
/// discriminant is not here and must not be: `docs/spec.md` makes a variant mean its enum
/// and its name, and the number a backend assigns is chosen fresh for each build from the
/// canonical order. A file that wrote the tag would be publishing a representation as a
/// meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentVariant {
    pub name: String,
    /// Its payload fields, sorted by name. Empty for a zero-payload variant.
    pub fields: Vec<PersistentField>,
}

/// One enum definition an importer needs, as published.
///
/// Closed over payload types the same way [`PersistentRecord`] is closed over field types,
/// and for the same reason: an importer has to understand every shape a public enum can
/// hand it, and cannot go and read the module that declared one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentEnum {
    /// Which logical enum this is: `module::enum Name`.
    pub def: String,
    pub name: String,
    /// How many type parameters it declares. Part of its interface: changing it changes
    /// what every use must write.
    pub generics: u32,
    /// Its variants, sorted by name.
    ///
    /// Sorted rather than in declaration order, so that moving two variant declarations
    /// produces identical bytes — which is what makes `docs/spec.md`'s claim that variant
    /// order is not semantic true of the fingerprint as well as of the language.
    pub variants: Vec<PersistentVariant>,
}

/// One record definition an importer needs, as published.
///
/// # Why some of these are not exported
///
/// The list is **closed over field types**: if an exported record has a field whose type
/// is a record from a third module, that record's shape is here too, because an importer
/// has to understand it to project through it and cannot go and read the module that
/// declared it. Which of them an importer may *name* is not a flag — it is whether
/// [`Self::def`] belongs to this interface's module, and it cannot be anything else: a
/// private type in a public surface is refused (`N0337`) before an interface is written,
/// so every record of this module that reaches this list is one it exports.
///
/// A generic record is published **as a definition**, with parameters in its field types.
/// No application of one — `Box[Int]` — is ever a record here: it is a type, written where
/// it is used, and the reader applies the definition itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentRecord {
    /// Which logical record this is: `module::record Name`.
    pub def: String,
    /// What a dependent writes to name it, when it may. Redundant with the tail of `def`
    /// and kept for the same reason [`PersistentExport::name`] is.
    pub name: String,
    /// How many type parameters it declares.
    pub generics: u32,
    /// Its fields, sorted by name.
    ///
    /// Sorted rather than in declaration order, so that moving two field declarations
    /// produces identical bytes — which is what makes `docs/spec.md`'s claim that field
    /// order is not semantic true of the fingerprint as well as of the language. A C struct's
    /// are in declaration order instead (N85): for it, order *is* semantic.
    pub fields: Vec<PersistentField>,
    /// What C sees of it (N85): absent for an ordinary record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<PersistentC>,
}

/// How a published record crosses the C boundary (N85).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersistentC {
    /// `extern "C" struct S { … }`.
    Struct,
    /// `extern "C" struct S;`.
    Handle,
}

impl PersistentC {
    fn of(repr: nazm_sema::CRepr) -> Option<Self> {
        match repr {
            nazm_sema::CRepr::Nazm => None,
            nazm_sema::CRepr::Layout => Some(Self::Struct),
            nazm_sema::CRepr::Opaque => Some(Self::Handle),
        }
    }

    fn repr(c: Option<Self>) -> nazm_sema::CRepr {
        match c {
            None => nazm_sema::CRepr::Nazm,
            Some(Self::Struct) => nazm_sema::CRepr::Layout,
            Some(Self::Handle) => nazm_sema::CRepr::Opaque,
        }
    }
}

/// One exported definition, as published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentExport {
    /// Which logical definition this is: `module::kind name`.
    pub def: String,
    /// What a dependent writes to name it.
    ///
    /// Redundant with the tail of `def`, and kept because a reader should not have to
    /// parse an identity to learn a name. The two are checked against each other on read.
    pub name: String,
    /// `fn`, today. Spelled as a word so that adding a second kind cannot reinterpret a
    /// value already written.
    pub kind: String,
    /// How many type parameters the function declares. Its **body** is not here, generic
    /// or not: a dependent checks against the signature and never needs the rest.
    pub generics: u32,
    /// The positions of the type parameters whose declaration requires equality, strictly
    /// ascending, and **absent** when there are none — so a function that requires nothing
    /// is written exactly as it was before N14. Positions, never names: renaming a
    /// parameter changes no byte, and a requirement belongs to the position as the
    /// parameter does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_equality: Vec<u32>,
    /// Each type parameter that requires a trait, and the trait by durable key (N78).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_traits: Vec<(u32, String)>,
    /// The parameter types, in declaration order. A parameter in them is this function's.
    pub params: Vec<PersistentType>,
    /// The return type, in the same encoding.
    pub ret: PersistentType,
    /// The declared effect set (N36), each effect by name, in effect-id order: `[]` for a
    /// function declaring `! {}`. **Absent** for one that declares none, which an importer
    /// reads as every effect — so absence and emptiness are different contracts, and are
    /// written differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<Vec<String>>,
    /// The C symbol of a foreign declaration (N42). **Absent** for a Nazm function, so a
    /// module with no foreign declaration is written exactly as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign: Option<String>,
    /// How many `requires` clauses it states (N87). **Absent** where it states none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub requires: u32,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// How a type is written in a persisted interface.
///
/// A built-in is its source name. A user-defined type is its definition's **durable
/// identity** — `module::record Name` — and never its source name, because the same
/// spelling may name a different record in every module. An application writes its
/// definition's identity and its arguments; a parameter writes its position.
fn persist(t: Type, types: &UserTypes, graph: &ModuleGraph) -> Option<PersistentType> {
    match t {
        Type::Param(p) => Some(PersistentType::Param { param: p.index }),
        Type::Vec(v) => Some(PersistentType::Vec {
            vec: Box::new(persist(types.vec_element(v)?, types, graph)?),
        }),
        Type::ChanOf(c) => Some(PersistentType::Chan {
            chan: Box::new(persist(types.chan_element(c)?, types, graph)?),
        }),
        Type::Fn(f) => {
            let sig = types.fn_signature(f)?;
            Some(PersistentType::Fn {
                params: sig
                    .params
                    .iter()
                    .map(|p| persist(*p, types, graph))
                    .collect::<Option<Vec<_>>>()?,
                ret: Box::new(persist(sig.ret, types, graph)?),
                effects: sig.effects.names().into_iter().map(str::to_owned).collect(),
            })
        }
        Type::Record(_) | Type::Enum(_) => {
            let node = types.definition_of(t)?;
            let def = definition_key(node, types, graph)?;
            let args = types
                .type_args(t)
                .into_iter()
                .map(|a| persist(a, types, graph))
                .collect::<Option<Vec<_>>>()?;
            Some(PersistentType::Def { def, args })
        }
        other => other
            .builtin_name()
            .map(|n| PersistentType::Builtin(n.to_owned())),
    }
}

/// The effect a persisted name means: a built-in's word, or `*` for the effect parameter
/// of the function whose signature it is in (N51).
fn effect_named(name: &str) -> Option<nazm_sema::Effect> {
    if name == nazm_sema::Effect::Param.name() {
        Some(nazm_sema::Effect::Param)
    } else {
        nazm_sema::Effect::by_name(name)
    }
}

/// A definition's durable identity, as a string.
fn definition_key(node: TypeNode, types: &UserTypes, graph: &ModuleGraph) -> Option<String> {
    match node {
        TypeNode::Record(id) => {
            let def = types.records().get(id)?;
            // The record's **own** module, which for a closure entry is not the module
            // publishing this interface. The tables hold a session-local `ModuleId`, so
            // the graph is what turns it into something durable.
            let key = graph.key(def.module)?;
            Some(DefKey::new(key.clone(), DefKind::Record, def.name.clone()).to_string())
        }
        TypeNode::Enum(id) => {
            let def = types.enums().get(id)?;
            let key = graph.key(def.module)?;
            Some(DefKey::new(key.clone(), DefKind::Enum, def.name.clone()).to_string())
        }
    }
}

/// A module's interface, as persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentUnitInterface {
    /// The shape these bytes are in. First field, so a reader can refuse before parsing
    /// the rest means anything.
    pub schema: String,
    /// Which logical module this is.
    pub module: String,
    /// The modules it directly imports, by durable key, sorted.
    ///
    /// **Direct only**, which is the rule `spec.md` settles. A dependency's own
    /// dependencies are its business and are not part of what this module offers.
    pub dependencies: Vec<String>,
    /// Every record definition its public surface mentions, sorted by durable identity.
    ///
    /// Before the exports, because an export's signature may refer to one of these and a
    /// reader should meet the definitions first.
    pub records: Vec<PersistentRecord>,
    /// Every enum definition its public surface mentions, sorted by durable identity.
    pub enums: Vec<PersistentEnum>,
    /// What it offers, sorted by name.
    pub exports: Vec<PersistentExport>,
    /// Its exported traits, sorted by durable identity (N78).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<PersistentTrait>,
    /// Every impl it declares, sorted by trait and then type (N78).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impls: Vec<PersistentImpl>,
}

/// An exported trait (N78). In each method's types, `{"param": 0}` is `Self`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentTrait {
    pub def: String,
    pub name: String,
    pub methods: Vec<PersistentTraitMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentTraitMethod {
    pub name: String,
    /// After `self`.
    pub params: Vec<PersistentType>,
    pub ret: PersistentType,
    pub effects: Vec<String>,
}

/// An impl a module declares (N78): which trait, for which type, and the function each
/// method is, by durable key, in the trait's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentImpl {
    #[serde(rename = "trait")]
    pub trait_def: String,
    #[serde(rename = "type")]
    pub self_ty: PersistentType,
    pub methods: Vec<String>,
}

/// Why a persisted interface could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The module has no durable identity — it escapes the source root, or a symbolic
    /// link gave it two names. `docs/spec.md`, *Durable identity*, says why that is an
    /// answer rather than a failure; there is simply nothing to write it under.
    NoModuleKey(ModuleId),
    /// A module it imports has none, and naming a dependency by a session-local id would
    /// be exactly the leak this format exists to prevent.
    DependencyWithoutKey(ModuleId),
    /// A user-defined type its public surface mentions is declared in a module with no
    /// durable identity, so there is nothing to write the field's type as. The same answer
    /// as the other two and for the same reason.
    RecordWithoutKey(ModuleId),
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoModuleKey(_) => f.write_str(
                "this module has no durable identity: it is outside the source root, or a \
                 symbolic link gave it more than one name",
            ),
            Self::DependencyWithoutKey(_) => f.write_str(
                "a module it imports has no durable identity, so this interface could not \
                 name its dependencies",
            ),
            Self::RecordWithoutKey(_) => f.write_str(
                "a type in its public surface is declared in a module with no durable \
                 identity, so this interface could not name that type",
            ),
        }
    }
}

/// Why a persisted interface could not be read.
///
/// Parsing is not trusting. Bytes that deserialise are still data written by something
/// else, and every one of these is a way a file could be well-formed JSON and not an
/// interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// Not JSON, or not this shape.
    Malformed(String),
    /// A schema this build does not implement. **Never** read as the current one: a
    /// future version means a reader written against this one would be wrong, which is
    /// what the version is for.
    UnknownSchema(String),
    /// A module or definition key that is not in the canonical spelling.
    BadKey(String),
    /// A type name this build does not have, or a type identity this interface does not
    /// define. A `/4` interface is **closed**: every record and enum a signature, a field
    /// or a payload names is defined in the same file, so a dangling reference is a
    /// malformed interface rather than something to go and look for.
    UnknownType(String),
    /// Two records, two enums, two fields or two variants under one name.
    DuplicateRecord(String),
    /// A definition kind this build does not have.
    UnknownKind(String),
    /// Two exports under one name, which no module can have.
    DuplicateExport(String),
    /// An export whose identity belongs to a different module than the interface's.
    ForeignExport(String),
    /// An export whose `name` and whose `def` disagree.
    NameMismatch(String),
    /// Dependencies or exports not in canonical order, which would mean two files for one
    /// interface and two fingerprints for one meaning.
    NotCanonical(String),
    /// A parameter position its owner does not have, or a parameter where no definition
    /// owns one. `/4`.
    BadParameter(String),
    /// A definition applied to a number of arguments it does not declare. `/4`.
    WrongArity(String),
    /// A name in an export's effect set that is not an effect. `/6`.
    UnknownEffect(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "not a `{SCHEMA}` interface: {e}"),
            Self::UnknownSchema(s) => write!(
                f,
                "interface schema `{s}` is not `{SCHEMA}`; this build cannot read it"
            ),
            Self::BadKey(k) => write!(f, "`{k}` is not a durable key in canonical form"),
            Self::UnknownType(t) => write!(f, "`{t}` is not a type this interface defines"),
            Self::DuplicateRecord(d) => write!(f, "`{d}` is defined twice"),
            Self::UnknownKind(k) => write!(f, "`{k}` is not a definition kind this build has"),
            Self::DuplicateExport(n) => write!(f, "`{n}` is exported twice"),
            Self::ForeignExport(d) => write!(f, "`{d}` is not a definition of this module"),
            Self::NameMismatch(d) => write!(f, "`{d}` disagrees with the name beside it"),
            Self::NotCanonical(w) => write!(f, "{w} are not in canonical order"),
            Self::BadParameter(w) => write!(f, "{w} names a type parameter that does not exist"),
            Self::WrongArity(d) => {
                write!(
                    f,
                    "`{d}` is applied to a number of arguments it does not declare"
                )
            }
            Self::UnknownEffect(w) => write!(f, "{w} is not an effect"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Whether a type may appear where `params` type parameters are in scope, against the
/// definitions this file declares and their arities.
fn validate_type(
    t: &PersistentType,
    params: u32,
    defined: &HashMap<&str, u32>,
    depth: usize,
    owner: &str,
) -> Result<(), ReadError> {
    if depth > MAX_TYPE_DEPTH * 2 {
        return Err(ReadError::Malformed(format!(
            "a type in {owner} nests deeper than any checked program could write"
        )));
    }
    match t {
        PersistentType::Builtin(name) => {
            if Type::from_name(name).is_none() {
                return Err(ReadError::UnknownType(name.clone()));
            }
        }
        PersistentType::Param { param } => {
            if *param >= params {
                return Err(ReadError::BadParameter(owner.to_owned()));
            }
        }
        PersistentType::Vec { vec } => validate_type(vec, params, defined, depth + 1, owner)?,
        PersistentType::Chan { chan } => {
            validate_type(chan, params, defined, depth + 1, owner)?;
        }
        PersistentType::Fn {
            params: ps,
            ret,
            effects,
        } => {
            for p in ps {
                validate_type(p, params, defined, depth + 1, owner)?;
            }
            validate_type(ret, params, defined, depth + 1, owner)?;
            // Known effects, each once, in effect-id order: the one spelling a set has.
            let mut last: Option<u32> = None;
            for e in effects {
                let Some(effect) = effect_named(e) else {
                    return Err(ReadError::Malformed(format!(
                        "`{e}` in a function type in {owner} is not an effect"
                    )));
                };
                let id = u32::from(effect.id());
                if last.is_some_and(|l| l >= id) {
                    return Err(ReadError::Malformed(format!(
                        "a function type's effects in {owner} are not in effect order"
                    )));
                }
                last = Some(id);
            }
        }
        PersistentType::Def { def, args } => {
            let Some(arity) = defined.get(def.as_str()) else {
                return Err(ReadError::UnknownType(def.clone()));
            };
            if args.len() != *arity as usize {
                return Err(ReadError::WrongArity(def.clone()));
            }
            for a in args {
                validate_type(a, params, defined, depth + 1, owner)?;
            }
        }
    }
    Ok(())
}

impl PersistentUnitInterface {
    /// Publish an in-memory interface.
    ///
    /// # Errors
    ///
    /// [`PublishError`] when this module or one it imports has no durable identity.
    pub fn published(
        interface: &UnitInterface,
        graph: &ModuleGraph,
        types: &UserTypes,
    ) -> Result<Self, PublishError> {
        let module = graph
            .key(interface.module())
            .ok_or(PublishError::NoModuleKey(interface.module()))?;

        let mut dependencies: Vec<String> = Vec::new();
        for d in interface.imports() {
            let key = graph
                .key(d.on)
                .ok_or(PublishError::DependencyWithoutKey(d.on))?;
            dependencies.push(key.as_str().to_owned());
        }
        // Sorted and deduplicated: the *set* of direct dependencies is what a module
        // depends on, and the order the loader happened to walk them in is not part of it.
        dependencies.sort();
        dependencies.dedup();

        // Every record the public surface mentions, each under its own module's key.
        let mut published_records: Vec<PersistentRecord> = Vec::new();
        for r in interface.records() {
            let Some(def) = types.records().get(r.def) else {
                continue;
            };
            let owner = graph
                .key(def.module)
                .ok_or(PublishError::RecordWithoutKey(def.module))?;
            let mut fields = Self::published_fields(&def.fields, types, graph, def.module)?;
            let c = PersistentC::of(def.repr);
            if c.is_some() {
                // A C struct's order is its layout (N85): declaration order, unsorted.
                fields.sort_by_key(|f| def.field(&f.name).map_or(u32::MAX, |(i, _)| i));
            }
            published_records.push(PersistentRecord {
                def: DefKey::new(owner.clone(), DefKind::Record, def.name.clone()).to_string(),
                name: def.name.clone(),
                generics: u32::try_from(def.params.len()).unwrap_or(u32::MAX),
                fields,
                c,
            });
        }
        // By durable identity, and once each: two paths through the public surface that
        // reach one record reach one definition.
        published_records.sort_by(|a, b| a.def.cmp(&b.def));
        published_records.dedup_by(|a, b| a.def == b.def);

        // Every enum the public surface mentions, by the same rule. Variants are sorted by
        // name, so the order they were declared in reaches no byte.
        let mut published_enums: Vec<PersistentEnum> = Vec::new();
        for e in interface.enums() {
            let Some(def) = types.enums().get(e.def) else {
                continue;
            };
            let owner = graph
                .key(def.module)
                .ok_or(PublishError::RecordWithoutKey(def.module))?;
            let mut variants: Vec<PersistentVariant> = Vec::new();
            for v in &def.variants {
                variants.push(PersistentVariant {
                    name: v.name.clone(),
                    fields: Self::published_fields(&v.fields, types, graph, def.module)?,
                });
            }
            variants.sort_by(|a, b| a.name.cmp(&b.name));
            published_enums.push(PersistentEnum {
                def: DefKey::new(owner.clone(), DefKind::Enum, def.name.clone()).to_string(),
                name: def.name.clone(),
                generics: u32::try_from(def.params.len()).unwrap_or(u32::MAX),
                variants,
            });
        }
        published_enums.sort_by(|a, b| a.def.cmp(&b.def));
        published_enums.dedup_by(|a, b| a.def == b.def);

        let mut exports: Vec<PersistentExport> = Vec::new();
        for e in interface.exports() {
            let mut params = Vec::with_capacity(e.params.len());
            for t in &e.params {
                // An exported signature naming a type from an unkeyed module has nothing
                // durable to write it as.
                params.push(
                    persist(*t, types, graph)
                        .ok_or_else(|| PublishError::NoModuleKey(interface.module()))?,
                );
            }
            exports.push(PersistentExport {
                def: DefKey::new(module.clone(), DefKind::Function, e.name.clone()).to_string(),
                name: e.name.clone(),
                kind: DefKind::Function.as_str().to_owned(),
                generics: u32::try_from(e.type_params.len()).unwrap_or(u32::MAX),
                requires_equality: (0..u32::try_from(e.type_params.len()).unwrap_or(u32::MAX))
                    .filter(|&index| {
                        types.requires_equality(TypeParam {
                            owner: ParamOwner::Function(e.def),
                            index,
                        })
                    })
                    .collect(),
                requires_traits: e
                    .bounds
                    .iter()
                    .filter_map(|(i, name, m)| {
                        let key = graph.key(*m)?;
                        Some((
                            *i,
                            DefKey::new(key.clone(), DefKind::Trait, name.clone()).to_string(),
                        ))
                    })
                    .collect(),
                params,
                ret: persist(e.ret, types, graph)
                    .ok_or_else(|| PublishError::NoModuleKey(interface.module()))?,
                effects: e
                    .effects
                    .map(|s| s.names().into_iter().map(str::to_owned).collect()),
                foreign: e.foreign.clone(),
                requires: e.preconditions,
            });
        }
        // By name: the order a module declares its functions in is not part of what it
        // offers, so reordering two definitions must not change a single byte.
        exports.sort_by(|a, b| a.name.cmp(&b.name));

        // Traits and impls (N78), each by durable identity.
        let trait_key = |name: &str, m: nazm_sema::ModuleId| -> Result<String, PublishError> {
            let key = graph.key(m).ok_or(PublishError::NoModuleKey(m))?;
            Ok(DefKey::new(key.clone(), DefKind::Trait, name.to_owned()).to_string())
        };
        let ty =
            |t: Type| persist(t, types, graph).ok_or(PublishError::NoModuleKey(interface.module()));
        let mut traits = Vec::new();
        for t in interface.traits() {
            let mut methods = Vec::new();
            for (name, params, ret, effects) in &t.methods {
                methods.push(PersistentTraitMethod {
                    name: name.clone(),
                    params: params.iter().map(|p| ty(*p)).collect::<Result<_, _>>()?,
                    ret: ty(*ret)?,
                    effects: effects.names().into_iter().map(str::to_owned).collect(),
                });
            }
            traits.push(PersistentTrait {
                def: trait_key(&t.name, t.module)?,
                name: t.name.clone(),
                methods,
            });
        }
        traits.sort_by(|a, b| a.def.cmp(&b.def));
        let mut impls = Vec::new();
        for i in interface.impls() {
            let trait_def = trait_key(&i.trait_name, i.trait_module)?;
            let self_ty = ty(i.self_ty)?;
            let shown = types.show(i.self_ty);
            impls.push(PersistentImpl {
                methods: i
                    .methods
                    .iter()
                    .map(|m| {
                        DefKey::new(
                            module.clone(),
                            DefKind::Function,
                            format!("{}/{shown}/{m}", i.trait_name),
                        )
                        .to_string()
                    })
                    .collect(),
                trait_def,
                self_ty,
            });
        }
        impls.sort_by(|a, b| {
            (
                a.trait_def.as_str(),
                serde_json::to_string(&a.self_ty).unwrap_or_default(),
            )
                .cmp(&(
                    b.trait_def.as_str(),
                    serde_json::to_string(&b.self_ty).unwrap_or_default(),
                ))
        });

        Ok(Self {
            schema: SCHEMA.to_owned(),
            module: module.as_str().to_owned(),
            dependencies,
            records: published_records,
            enums: published_enums,
            exports,
            traits,
            impls,
        })
    }

    /// One field list, sorted by name. Shared by a record's fields and a variant's payload,
    /// because they are the same thing and must publish the same way.
    fn published_fields(
        fields: &[FieldDef],
        types: &UserTypes,
        graph: &ModuleGraph,
        owner: nazm_sema::ModuleId,
    ) -> Result<Vec<PersistentField>, PublishError> {
        let mut out: Vec<PersistentField> = Vec::new();
        for f in fields {
            let ty = persist(f.ty, types, graph).ok_or(PublishError::RecordWithoutKey(owner))?;
            out.push(PersistentField {
                name: f.name.clone(),
                ty,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The canonical bytes.
    ///
    /// # Errors
    ///
    /// Only if `serde_json` cannot serialise one of these types, which would be a defect.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Read a persisted interface, refusing anything that is not one this build writes.
    ///
    /// # Errors
    ///
    /// [`ReadError`], for a malformed file, an unknown schema, or bytes that deserialise
    /// but do not describe a valid interface.
    pub fn from_json(text: &str) -> Result<Self, ReadError> {
        // The schema first, and by itself, so a future version is refused as *unknown*
        // rather than as whatever structural mismatch its new fields happen to cause.
        #[derive(Deserialize)]
        struct Envelope {
            schema: String,
        }
        let envelope: Envelope =
            serde_json::from_str(text).map_err(|e| ReadError::Malformed(e.to_string()))?;
        if envelope.schema != SCHEMA {
            return Err(ReadError::UnknownSchema(envelope.schema));
        }

        let parsed: Self =
            serde_json::from_str(text).map_err(|e| ReadError::Malformed(e.to_string()))?;
        parsed.validate()?;
        // And written the way this build writes it. The structural checks above catch the
        // orderings a reader could get wrong; this catches every other way two files could
        // describe one interface — a field the reader ignored, an empty argument list
        // spelled differently — because two files for one meaning are two fingerprints.
        let canonical = parsed
            .to_json()
            .map_err(|e| ReadError::Malformed(e.to_string()))?;
        if canonical != text.trim_end_matches('\n') {
            return Err(ReadError::NotCanonical("its bytes".to_owned()));
        }
        Ok(parsed)
    }

    /// Everything a well-formed interface must be, beyond deserialising.
    fn validate(&self) -> Result<(), ReadError> {
        let module =
            ModuleKey::parse(&self.module).ok_or_else(|| ReadError::BadKey(self.module.clone()))?;

        let mut deps = Vec::with_capacity(self.dependencies.len());
        for d in &self.dependencies {
            deps.push(ModuleKey::parse(d).ok_or_else(|| ReadError::BadKey(d.clone()))?);
        }
        if deps.windows(2).any(|w| w.first() >= w.last()) {
            return Err(ReadError::NotCanonical("dependencies".to_owned()));
        }

        // Every definition this file declares, with its arity. Records and enums share the
        // list — a type key names its kind — so a field may refer to either.
        let mut defined: HashMap<&str, u32> = HashMap::new();
        let mut previous_record: Option<&str> = None;
        for r in &self.records {
            let key = parse_type_key(&r.def, DefKind::Record)
                .ok_or_else(|| ReadError::BadKey(r.def.clone()))?;
            if key.name() != r.name {
                return Err(ReadError::NameMismatch(r.def.clone()));
            }
            match previous_record {
                Some(p) if p == r.def => return Err(ReadError::DuplicateRecord(r.def.clone())),
                Some(p) if p > r.def.as_str() => {
                    return Err(ReadError::NotCanonical("records".to_owned()))
                }
                _ => {}
            }
            previous_record = Some(&r.def);
            defined.insert(&r.def, r.generics);
            canonical_fields(&r.def, &r.fields)?;
        }
        let mut previous_enum: Option<&str> = None;
        for e in &self.enums {
            let key = parse_type_key(&e.def, DefKind::Enum)
                .ok_or_else(|| ReadError::BadKey(e.def.clone()))?;
            if key.name() != e.name {
                return Err(ReadError::NameMismatch(e.def.clone()));
            }
            match previous_enum {
                Some(p) if p == e.def => return Err(ReadError::DuplicateRecord(e.def.clone())),
                Some(p) if p > e.def.as_str() => {
                    return Err(ReadError::NotCanonical("enums".to_owned()))
                }
                _ => {}
            }
            previous_enum = Some(&e.def);
            defined.insert(&e.def, e.generics);
            let mut previous_variant: Option<&str> = None;
            for v in &e.variants {
                match previous_variant {
                    Some(p) if p == v.name => {
                        return Err(ReadError::DuplicateRecord(format!("{}.{}", e.def, v.name)))
                    }
                    Some(p) if p > v.name.as_str() => {
                        return Err(ReadError::NotCanonical("variants".to_owned()))
                    }
                    _ => {}
                }
                previous_variant = Some(&v.name);
                canonical_fields(&format!("{}.{}", e.def, v.name), &v.fields)?;
            }
        }
        // Every type a field or payload names, in the scope of its own definition's
        // parameters: a `{"param":1}` in a one-parameter record is a file that disagrees
        // with itself.
        for r in &self.records {
            for f in &r.fields {
                validate_type(&f.ty, r.generics, &defined, 0, &r.def)?;
            }
        }
        for e in &self.enums {
            for v in &e.variants {
                for f in &v.fields {
                    validate_type(&f.ty, e.generics, &defined, 0, &e.def)?;
                }
            }
        }

        let mut previous: Option<&str> = None;
        for e in &self.exports {
            let kind =
                DefKind::parse(&e.kind).ok_or_else(|| ReadError::UnknownKind(e.kind.clone()))?;
            if kind != DefKind::Function {
                return Err(ReadError::UnknownKind(e.kind.clone()));
            }
            for t in e.params.iter().chain(std::iter::once(&e.ret)) {
                validate_type(t, e.generics, &defined, 0, &e.def)?;
            }
            // Each a position the function has, once, in ascending order: anything else is
            // a file that disagrees with itself or was not written by the canonical writer.
            if e.requires_equality
                .windows(2)
                .any(|w| w.first() >= w.last())
                || e.requires_equality.iter().any(|&p| p >= e.generics)
            {
                return Err(ReadError::NotCanonical(format!(
                    "the requirements of `{}`",
                    e.def
                )));
            }
            // Each name an effect, once, in id order: the one canonical spelling of a set.
            if let Some(names) = &e.effects {
                let mut last: Option<u8> = None;
                for n in names {
                    let effect = effect_named(n).ok_or_else(|| {
                        ReadError::UnknownEffect(format!("`{n}` in the effects of `{}`", e.def))
                    })?;
                    if last.is_some_and(|l| l >= effect.id()) {
                        return Err(ReadError::NotCanonical(format!(
                            "the effects of `{}`",
                            e.def
                        )));
                    }
                    last = Some(effect.id());
                }
            }
            // The identity must be this module's, and must be the one the name and kind
            // beside it describe. Either mismatch means the file disagrees with itself.
            let expected = DefKey::new(module.clone(), kind, e.name.clone()).to_string();
            if e.def != expected {
                if e.def.starts_with(&format!("{module}::")) {
                    return Err(ReadError::NameMismatch(e.def.clone()));
                }
                return Err(ReadError::ForeignExport(e.def.clone()));
            }
            match previous {
                Some(p) if p == e.name => return Err(ReadError::DuplicateExport(e.name.clone())),
                Some(p) if p > e.name.as_str() => {
                    return Err(ReadError::NotCanonical("exports".to_owned()))
                }
                _ => {}
            }
            previous = Some(&e.name);
        }
        Ok(())
    }

    /// Rebuild an in-memory interface from this one, for **checking a dependent**.
    ///
    /// `module` is the id this interface is bound to in the compilation about to use it,
    /// and `first` is where its definitions are numbered from. Both are session-local and
    /// both are minted by the caller: this file contains no ids and could not, which is
    /// the point of it.
    ///
    /// # What the rebuilt value does and does not carry
    ///
    /// The names, the signatures and the visibility implied by being here — everything
    /// `nazm_core::check_unit` reads. Three things are placeholders and are named as such:
    ///
    /// * **spans** are [`Span::synthetic`](nazm_span::Span::synthetic). A diagnostic about
    ///   an imported definition would normally point into the exporting file, and that
    ///   file is not open — the whole point being that a dependent needs no dependency
    ///   source. So the secondary label points nowhere rather than somewhere wrong;
    /// * **type parameter names** are not persisted, so a message about one says where it
    ///   is — "type parameter 1" — rather than what it was called;
    /// * the **dependency list is empty**. Checking a dependent never reads a
    ///   dependency's own dependencies — that is the non-transitivity rule, expressed as
    ///   data — so nothing is lost.
    ///
    /// Each call gives every type the interface carries an identity of its own. A
    /// compilation that rehydrates **several** interfaces, where one mentions a type
    /// another declares, uses [`Self::rehydrate_into`] so the two agree it is one type.
    #[must_use]
    pub fn rehydrate(&self, module: ModuleId, first: FnId, types: &mut UserTypes) -> UnitInterface {
        self.rehydrate_into(module, first, types, &mut Rehydrated::default())
    }

    /// As [`Self::rehydrate`], sharing type identities with every interface already
    /// rehydrated through `seen`.
    ///
    /// # Why this exists
    ///
    /// An interface carries every type its exported shapes mention, including ones other
    /// modules declare: B's `pub fn parse(…) -> Result[Int, E]` carries the core prelude's
    /// `Result`. Rehydrated alone, that is a fresh definition bound to B, and a dependent
    /// that also rehydrated the prelude would hold two `Result`s that are not equal —
    /// `parse(s)?` would be a `?` on an enum that is not the core one. Keyed by
    /// **durable identity** (`DefKey`), a definition met in two interfaces is one
    /// definition, which is what it is.
    ///
    /// The order does not matter. A definition first met as another interface's
    /// mention and later as its own module's export is adopted by that module then.
    #[must_use]
    pub fn rehydrate_into(
        &self,
        module: ModuleId,
        first: FnId,
        types: &mut UserTypes,
        seen: &mut Rehydrated,
    ) -> UnitInterface {
        // Two passes, for the reason the checker declares type names before resolving
        // field types: a field may name a type defined later in this file, and both
        // orders have to work. And applications met while reading field types wait for
        // every definition to have its fields, exactly as in a compilation.
        types.defer();
        let mut ids: HashMap<&str, TypeNode> = HashMap::new();
        let own = self.module_key();
        // A type this interface's module declares is one an importer may name. One from
        // another module is here because a shape mentions it, and naming it would be the
        // transitive visibility `docs/spec.md` refuses — so it is declared **without**
        // registering its name.
        let mine = |def: &str, kind: DefKind| {
            parse_type_key(def, kind)
                .zip(own.as_ref())
                .is_some_and(|(k, m)| k.module() == m)
        };
        let placeholder = |n: u32| vec![String::new(); n as usize];
        // The definitions this call declares, and so the ones whose fields it fills. One
        // another interface already brought in has its fields already.
        let mut fresh: Vec<&str> = Vec::new();
        for r in &self.records {
            let nameable = mine(&r.def, DefKind::Record);
            if let Some(TypeNode::Record(id)) = seen.known.get(&r.def).copied() {
                if nameable {
                    types.records_mut().adopt(id, module);
                }
                ids.insert(&r.def, TypeNode::Record(id));
                continue;
            }
            let def = RecordDef {
                name: r.name.clone(),
                module,
                visibility: if nameable {
                    Visibility::Exported
                } else {
                    Visibility::Private
                },
                // No span: the exporting file is not open, which is the point. A
                // diagnostic about an imported type points nowhere rather than somewhere
                // wrong.
                name_span: Span::synthetic(),
                fields: Vec::new(),
                layout: Vec::new(),
                // No declaration to go back to. Nothing reads `decl` for a type that came
                // from an interface, because there is no tree it indexes into.
                decl: u32::MAX,
                params: placeholder(r.generics),
                instance: None,
                repr: PersistentC::repr(r.c),
            };
            let id = if nameable {
                types.records_mut().declare(def)
            } else {
                types.records_mut().declare_unnamed(def)
            };
            ids.insert(&r.def, TypeNode::Record(id));
            seen.known.insert(r.def.clone(), TypeNode::Record(id));
            fresh.push(&r.def);
        }
        for e in &self.enums {
            let nameable = mine(&e.def, DefKind::Enum);
            if let Some(TypeNode::Enum(id)) = seen.known.get(&e.def).copied() {
                if nameable {
                    types.enums_mut().adopt(id, module);
                }
                ids.insert(&e.def, TypeNode::Enum(id));
                continue;
            }
            let def = EnumDef {
                name: e.name.clone(),
                module,
                visibility: if nameable {
                    Visibility::Exported
                } else {
                    Visibility::Private
                },
                name_span: Span::synthetic(),
                variants: Vec::new(),
                order: Vec::new(),
                decl: u32::MAX,
                params: placeholder(e.generics),
                instance: None,
            };
            let id = if nameable {
                types.enums_mut().declare(def)
            } else {
                types.enums_mut().declare_unnamed(def)
            };
            ids.insert(&e.def, TypeNode::Enum(id));
            seen.known.insert(e.def.clone(), TypeNode::Enum(id));
            fresh.push(&e.def);
        }
        for r in &self.records {
            if !fresh.contains(&r.def.as_str()) {
                continue;
            }
            let Some(TypeNode::Record(id)) = ids.get(r.def.as_str()).copied() else {
                continue;
            };
            let owner = ParamOwner::Record(id);
            let fields = read_fields(&r.fields, &ids, types, owner);
            types.records_mut().complete(id, fields);
        }
        for e in &self.enums {
            if !fresh.contains(&e.def.as_str()) {
                continue;
            }
            let Some(TypeNode::Enum(id)) = ids.get(e.def.as_str()).copied() else {
                continue;
            };
            let owner = ParamOwner::Enum(id);
            let variants = e
                .variants
                .iter()
                .map(|v| VariantDef {
                    name: v.name.clone(),
                    name_span: Span::synthetic(),
                    fields: read_fields(&v.fields, &ids, types, owner),
                    layout: Vec::new(),
                })
                .collect();
            types.enums_mut().complete(id, variants);
        }

        let mut exports = Vec::with_capacity(self.exports.len());
        for (i, e) in self.exports.iter().enumerate() {
            let def = FnId::from_index(first.index() + i);
            let owner = ParamOwner::Function(def);
            exports.push(Export {
                // A persisted interface's traits are not rehydrated (N78): a dependent that
                // needs one is checked against source, or refused, never misread.
                bounds: Vec::new(),
                name: e.name.clone(),
                params: e
                    .params
                    .iter()
                    .map(|t| read_type(t, &ids, types, owner))
                    .collect(),
                ret: read_type(&e.ret, &ids, types, owner),
                name_span: Span::synthetic(),
                def,
                type_params: placeholder(e.generics),
                // Validated on read: every name is an effect, or `*` for the parameter.
                effects: e
                    .effects
                    .as_ref()
                    .map(|names| names.iter().filter_map(|n| effect_named(n)).collect()),
                foreign: e.foreign.clone(),
                preconditions: e.requires,
            });
            for &index in &e.requires_equality {
                types.require_equality(TypeParam { owner, index });
            }
        }
        types.settle();
        let record_exports = self
            .records
            .iter()
            .filter_map(|r| match ids.get(r.def.as_str()).copied() {
                Some(TypeNode::Record(def)) => Some(RecordExport {
                    def,
                    nameable: mine(&r.def, DefKind::Record),
                }),
                _ => None,
            })
            .collect();
        let enum_exports = self
            .enums
            .iter()
            .filter_map(|e| match ids.get(e.def.as_str()).copied() {
                Some(TypeNode::Enum(def)) => Some(EnumExport {
                    def,
                    nameable: mine(&e.def, DefKind::Enum),
                }),
                _ => None,
            })
            .collect();
        UnitInterface::new(module, exports, record_exports, enum_exports, Vec::new())
    }

    /// This module's durable identity. Validated on read, so this cannot fail afterwards.
    #[must_use]
    pub fn module_key(&self) -> Option<ModuleKey> {
        ModuleKey::parse(&self.module)
    }
}

/// The type definitions a compilation has rehydrated so far, by durable identity.
///
/// Threaded through [`PersistentUnitInterface::rehydrate_into`] so that a definition two
/// interfaces carry is rehydrated once. Session-local, like the ids it holds.
#[derive(Debug, Default)]
pub struct Rehydrated {
    known: HashMap<String, TypeNode>,
}

/// Split `module::record Name` or `module::enum Name` back into a key.
///
/// Written out rather than given to `DefKey` as a parser, because a `DefKey` is a value a
/// compilation computes and this is the one place a *file* claims to be one. Anything that
/// is not in the canonical spelling is refused rather than repaired, and a key whose kind
/// is not the one the caller is reading is refused rather than accepted under the wrong
/// heading — an enum listed among the records is a file that disagrees with itself.
fn parse_type_key(text: &str, want: DefKind) -> Option<DefKey> {
    let (module, rest) = text.split_once("::")?;
    let (kind, name) = rest.split_once(' ')?;
    if DefKind::parse(kind)? != want {
        return None;
    }
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    Some(DefKey::new(ModuleKey::parse(module)?, want, name))
}

/// A published field list must be sorted by name with no duplicate.
///
/// `owner` is only for the message. Checked rather than assumed, because a file is data:
/// bytes that deserialise are still something else's output, and two orderings of one
/// field set would be two files for one interface and two fingerprints for one meaning.
fn canonical_fields(owner: &str, fields: &[PersistentField]) -> Result<(), ReadError> {
    let mut previous: Option<&str> = None;
    for f in fields {
        match previous {
            Some(p) if p == f.name => {
                return Err(ReadError::DuplicateRecord(format!("{owner}.{}", f.name)))
            }
            Some(p) if p > f.name.as_str() => {
                return Err(ReadError::NotCanonical("fields".to_owned()))
            }
            _ => {}
        }
        previous = Some(&f.name);
    }
    Ok(())
}

/// A published field list, as this compilation's fields.
fn read_fields(
    fields: &[PersistentField],
    ids: &HashMap<&str, TypeNode>,
    types: &mut UserTypes,
    owner: ParamOwner,
) -> Vec<FieldDef> {
    fields
        .iter()
        .map(|f| FieldDef {
            name: f.name.clone(),
            ty: read_type(&f.ty, ids, types, owner),
            name_span: Span::synthetic(),
            ty_span: Span::synthetic(),
        })
        .collect()
}

/// A published type, as a [`Type`] in the reading compilation: applications interned, and
/// each parameter the one `owner` declares at that position.
///
/// Validation has already established that every name is either a built-in or one of this
/// file's types, and every arity right, so the fallbacks are unreachable; they are `Int`
/// rather than a panic for the same reason the checker continues as `Int` after an unknown
/// type.
fn read_type(
    t: &PersistentType,
    ids: &HashMap<&str, TypeNode>,
    types: &mut UserTypes,
    owner: ParamOwner,
) -> Type {
    match t {
        PersistentType::Builtin(name) => Type::from_name(name).unwrap_or(Type::Int),
        PersistentType::Param { param } => Type::Param(TypeParam {
            owner,
            index: *param,
        }),
        PersistentType::Vec { vec } => {
            let elem = read_type(vec, ids, types, owner);
            types.vec_of(elem)
        }
        PersistentType::Chan { chan } => {
            let elem = read_type(chan, ids, types, owner);
            types.chan_of(elem)
        }
        PersistentType::Fn {
            params,
            ret,
            effects,
        } => {
            let params = params
                .iter()
                .map(|p| read_type(p, ids, types, owner))
                .collect();
            let ret = read_type(ret, ids, types, owner);
            let effects = effects
                .iter()
                .filter_map(|e| effect_named(e))
                .fold(nazm_sema::EffectSet::EMPTY, nazm_sema::EffectSet::with);
            // `*` in a published signature is the publishing function's parameter (N51).
            types.fn_of(nazm_sema::FnSignature {
                params,
                ret,
                effects,
                row: Some(owner),
            })
        }
        PersistentType::Def { def, args } => {
            let Some(node) = ids.get(def.as_str()).copied() else {
                return Type::Int;
            };
            if args.is_empty() {
                return node.ty();
            }
            let args: Vec<Type> = args
                .iter()
                .map(|a| read_type(a, ids, types, owner))
                .collect();
            match node {
                TypeNode::Record(id) => types.apply_record(id, args),
                TypeNode::Enum(id) => types.apply_enum(id, args),
            }
        }
    }
}
