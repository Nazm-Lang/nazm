//! The representation itself.
//!
//! `docs/architecture.md` §7.42 is the law this file follows. The short version: every concrete
//! function is a graph of basic blocks; every block ends in exactly one terminator; every value
//! of a managed type is moved, copied or dropped by a statement that says so; and every
//! operation that can fail names the block its failure goes to.

use nazm_sema::key::DefKey;
use nazm_sema::{EffectSet, EnumId, FnId, Intrinsic, ModuleId, RecordId, Type};
use nazm_span::Span;

pub use nazm_cir::{ArithOp, BitOp, CmpOp, Law};
pub use nazm_sema::{ConvertMode, NumKind};

/// A native program: every concrete function it contains, the concrete types they use, and
/// where it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// By [`FuncId`]: non-generic definitions in `FnId` order, then generic instances in
    /// canonical-key order.
    pub functions: Vec<Function>,
    /// Every module that declares a function — generic or not — in load order, with its
    /// file. A module whose only functions are generic still has an artefact of its own, the
    /// same whoever instantiates what.
    pub modules: Vec<(ModuleId, nazm_span::FileId)>,
    pub types: Types,
    pub entry: Option<FuncId>,
    /// Every static (Gate 3), in the order the checker recorded them.
    pub statics: Vec<Static>,
}

impl Program {
    #[must_use]
    pub fn function(&self, id: FuncId) -> Option<&Function> {
        self.functions.get(id.index())
    }
}

/// A MIR function, by its position in [`Program::functions`]. Ephemeral: one lowering's
/// numbering, never persisted and never a durable identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncId(pub u32);

impl FuncId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Who a definition is, durably: its key where its module has one, and otherwise its module's
/// load position and its name — which every durable form of an unkeyed module uses and says so.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    Keyed(DefKey),
    Unkeyed { module: ModuleId, name: String },
}

/// A concrete function's identity: its definition, and — for a generic instance — the
/// canonical keys of its type arguments. What a backend derives a symbol from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identity {
    pub owner: Owner,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub id: FuncId,
    /// The checker's definition this is, or is an instance of.
    pub def: FnId,
    /// The type arguments of an instance, concrete; empty for a non-generic definition.
    pub type_args: Vec<Type>,
    pub module: ModuleId,
    /// Display only.
    pub name: String,
    pub identity: Identity,
    /// Whether the definition is `pub`. A linkage input, decided by the language.
    pub exported: bool,
    /// Whether a generic definition's body in its own module calls this function, so that the
    /// instances generated from that body — artefacts of their own — must be able to bind to
    /// it. A fact about the definitions, never about who instantiates them.
    pub reached_from_generic: bool,
    pub params: u32,
    /// Parameters first, then the checker's bindings, then temporaries.
    pub locals: Vec<LocalDecl>,
    pub ret: Type,
    /// `BlockId(0)` is the entry.
    pub blocks: Vec<Block>,
    /// The published effect set. Metadata: nothing below MIR reads or checks it.
    pub effects: EffectSet,
    pub span: Span,
    /// Where the result type is written, for a diagnostic about it.
    pub ret_span: Span,
    /// The C symbol of a foreign function (N42). A foreign function has its parameters and no
    /// blocks: its body is C's, and a backend declares it and calls it with the C convention.
    pub foreign: Option<String>,
    /// The C symbol an exported function is also callable as (N55).
    pub export: Option<String>,
    /// The section `@section` places it in (Gate 3): a board's image only.
    pub section: Option<String>,
    /// The interrupt `@interrupt` makes it the handler of (Gate 3): a board's image only.
    pub interrupt: Option<String>,
    /// Whether `@on_failure` makes it the board's failure hook (Gate 3).
    pub on_failure: bool,
    /// `@task`'s priority and period (Gate 3): a board's periodic task.
    pub task: Option<(u32, u64)>,
    /// For the code of a function value — a closure's body, or a named function's thunk
    /// (N50): the types of what the closure object carries, in order. Its local 0, a
    /// parameter, is the closure object; its declared parameters follow. `None` for every
    /// function called directly.
    pub closure: Option<Vec<Type>>,
}

impl Function {
    #[must_use]
    pub fn local(&self, id: LocalId) -> Option<&LocalDecl> {
        self.locals.get(id.index())
    }

    #[must_use]
    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(id.index())
    }

    /// The parameters' types, in order.
    pub fn param_types(&self) -> impl Iterator<Item = Type> + '_ {
        self.locals
            .iter()
            .take(self.params as usize)
            .filter_map(|l| l.ty.value())
    }

    /// How many capability parameters this function takes — the roots the platform entry hands
    /// `main`.
    #[must_use]
    pub fn capability_params(&self) -> usize {
        self.param_types().filter(|t| t.is_capability()).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

impl LocalId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

impl BlockId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A local's type: a concrete checker type, or the list of tasks a `scope` is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalTy {
    Value(Type),
    /// Internal to MIR: no source expression has this type. Opened by
    /// [`StatementKind::ScopeOpen`] and consumed only by [`StatementKind::ScopeJoin`].
    Tasks,
}

impl LocalTy {
    #[must_use]
    pub const fn value(self) -> Option<Type> {
        match self {
            Self::Value(t) => Some(t),
            Self::Tasks => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalOrigin {
    Param,
    Binding {
        mutable: bool,
    },
    /// Made by MIR lowering to hold an intermediate value.
    Temp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDecl {
    pub ty: LocalTy,
    /// Display only.
    pub name: String,
    pub origin: LocalOrigin,
    pub at: Span,
    /// The source block a binding is visible in — the innermost `{…}` its `let` is in — for a
    /// debugger's lexical scopes (N91). `None` for a parameter or a binding in the function's
    /// own body, visible in the whole function, and for a temporary. Display only: no digest and no printed form includes it.
    pub scope: Option<Span>,
}

/// Why a block exists. Compiler-made blocks say so, which is what keeps a debugger or a reader
/// from taking a cleanup edge for source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOrigin {
    Source,
    /// Joins the scopes open at a failing operation, on the way to the unwind block.
    FailureCleanup,
    /// The function's one failure exit: drops every managed local and unwinds.
    Unwind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Statement>,
    pub term: Terminator,
    /// The span the terminator came from, or a synthetic span for a compiler-made one.
    pub term_at: Span,
    pub origin: BlockOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub kind: StatementKind,
    /// The span a failure of this statement is reported at, or a synthetic span for a
    /// compiler-made statement. For a call — whose failures are the callee's, reported at the
    /// callee's own positions — it is where the callee is named, which is what a diagnostic
    /// about the call (a refused recursion) points at.
    pub at: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKind {
    /// `dst = value`. `dst` must be empty if its type is managed. `unwind` is present exactly
    /// when the rvalue can fail.
    Assign {
        dst: LocalId,
        value: Rvalue,
        unwind: Option<BlockId>,
    },
    /// `base.f₁.….fₙ = value`, where `value` is **moved** in and the field's old value is
    /// released. `base` is a binding of record type.
    SetField {
        base: LocalId,
        path: Vec<FieldRef>,
        value: LocalId,
    },
    /// `base.path = value` where the path passes through an array element (Gate 2): each
    /// index is a local read before anything is written, checked against its array's length,
    /// and a failure leaves by `unwind`. Arrays hold plain data, so nothing is released.
    SetElement {
        base: LocalId,
        path: Vec<PathRef>,
        value: LocalId,
        unwind: BlockId,
    },
    /// Give back what `local` owns and leave it empty. A no-op on an empty local.
    Drop(LocalId),
    /// Allocate the task list a `scope` holds, into `list`.
    ScopeOpen { list: LocalId, unwind: BlockId },
    /// Start a task running `callee` in the scope whose list is `list`. The arguments are
    /// borrowed; the task takes a reference of its own to each and gives it back when it ends.
    Spawn {
        list: LocalId,
        callee: FuncId,
        args: Vec<Operand>,
        unwind: BlockId,
        /// Where the callee is named, for a diagnostic about the call itself.
        callee_at: Span,
    },
    /// Wait for every task in `list`, adopt the first failure any had, and release the list.
    ScopeJoin { list: LocalId },
    /// If this thread has failed — a callee, a task adopted by a join — leave along `unwind`.
    CheckFailed { unwind: BlockId },
    /// If this thread's stack is nearly exhausted, fail with `N0408` and leave along `unwind`
    /// (N49, `docs/spec.md` *Native recursion*). First in every function in a call cycle.
    StackCheck { unwind: BlockId },
}

/// One step of a field path: which record, and which of its fields by declaration index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldRef {
    pub record: RecordId,
    pub field: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    Goto(BlockId),
    Branch {
        cond: Operand,
        then: BlockId,
        otherwise: BlockId,
    },
    /// On the variant `scrutinee` holds. One arm per variant, each variant exactly once, and
    /// no default: exhaustiveness was the checker's, and a backend repeats none of it.
    Switch {
        scrutinee: LocalId,
        enumeration: EnumId,
        arms: Vec<(u32, BlockId)>,
    },
    /// Move `value` out to the caller.
    Return(LocalId),
    /// The failure exit: the thread's failure is recorded and the caller will see it.
    Unwind,
    /// `exit_with`: the process ends here, with this status and no cleanup.
    Exit(Operand),
}

/// The locals an operand names.
fn operand_locals(o: &Operand, out: &mut Vec<LocalId>) {
    if let Operand::Local(l) = o {
        out.push(*l);
    }
}

impl Rvalue {
    /// Every local this rvalue reads, in order (N59): a backend deciding how to represent a
    /// local needs every place it is mentioned, and this is the one list of them.
    #[must_use]
    pub fn reads(&self) -> Vec<LocalId> {
        let mut out = Vec::new();
        match self {
            Self::Use(o)
            | Self::Neg(o)
            | Self::Complement(o)
            | Self::Convert { operand: o, .. }
            | Self::Fits { operand: o, .. }
            | Self::Hash { value: o, .. } => operand_locals(o, &mut out),
            Self::Copy(l) | Self::Move(l) | Self::Capture { env: l, .. } => out.push(*l),
            Self::Array { elems } => {
                for a in elems {
                    operand_locals(a, &mut out);
                }
            }
            Self::ArrayRepeat { value: a, .. } => operand_locals(a, &mut out),
            Self::Arith(_, a, b)
            | Self::Bits(_, a, b)
            | Self::Index { base: a, index: b }
            | Self::Compare { lhs: a, rhs: b, .. } => {
                operand_locals(a, &mut out);
                operand_locals(b, &mut out);
            }
            Self::Call { args, .. } | Self::Primitive { args, .. } => {
                for a in args {
                    operand_locals(a, &mut out);
                }
            }
            Self::CallIndirect { callee, args } => {
                operand_locals(callee, &mut out);
                for a in args {
                    operand_locals(a, &mut out);
                }
            }
            Self::Closure { captures, .. } => {
                for a in captures {
                    operand_locals(a, &mut out);
                }
            }
            Self::Record { fields, .. } | Self::Variant { fields, .. } => {
                out.extend(fields.iter().map(|(_, l)| *l));
            }
            Self::Field { base, .. } => operand_locals(base, &mut out),
            Self::Payload { base, .. } => out.push(*base),
        }
        out
    }
}

impl StatementKind {
    /// Every local this statement mentions other than the one an `Assign` defines (N59).
    #[must_use]
    pub fn mentions(&self) -> Vec<LocalId> {
        match self {
            Self::Assign { value, .. } => value.reads(),
            Self::SetField { base, value, .. } => vec![*base, *value],
            Self::SetElement {
                base, path, value, ..
            } => {
                let mut out = vec![*base];
                for p in path {
                    if let PathRef::Index { index, .. } = p {
                        out.push(*index);
                    }
                }
                out.push(*value);
                out
            }
            Self::Drop(l) | Self::ScopeOpen { list: l, .. } | Self::ScopeJoin { list: l } => {
                vec![*l]
            }
            Self::Spawn { list, args, .. } => {
                let mut out = vec![*list];
                for a in args {
                    operand_locals(a, &mut out);
                }
                out
            }
            Self::CheckFailed { .. } | Self::StackCheck { .. } => Vec::new(),
        }
    }
}

impl Terminator {
    /// Every local this terminator reads (N59).
    #[must_use]
    pub fn reads(&self) -> Vec<LocalId> {
        let mut out = Vec::new();
        match self {
            Self::Branch { cond, .. } => operand_locals(cond, &mut out),
            Self::Switch { scrutinee, .. } | Self::Return(scrutinee) => out.push(*scrutinee),
            Self::Exit(o) => operand_locals(o, &mut out),
            Self::Goto(_) | Self::Unwind => {}
        }
        out
    }

    /// Every block this terminator may transfer to, in a fixed order.
    #[must_use]
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Self::Goto(b) => vec![*b],
            Self::Branch {
                then, otherwise, ..
            } => vec![*then, *otherwise],
            Self::Switch { arms, .. } => arms.iter().map(|(_, b)| *b).collect(),
            Self::Return(_) | Self::Unwind | Self::Exit(_) => Vec::new(),
        }
    }
}

impl StatementKind {
    /// The failure edge this statement carries, if it can fail.
    #[must_use]
    pub const fn unwind(&self) -> Option<BlockId> {
        match self {
            Self::Assign { unwind, .. } => *unwind,
            Self::ScopeOpen { unwind, .. }
            | Self::Spawn { unwind, .. }
            | Self::CheckFailed { unwind }
            | Self::StackCheck { unwind }
            | Self::SetElement { unwind, .. } => Some(*unwind),
            Self::SetField { .. } | Self::Drop(_) | Self::ScopeJoin { .. } => None,
        }
    }
}

impl Block {
    /// Every block control may reach from this one: failure edges, then the terminator's.
    #[must_use]
    pub fn successors(&self) -> Vec<BlockId> {
        let mut out: Vec<BlockId> = self.stmts.iter().filter_map(|s| s.kind.unwind()).collect();
        out.extend(self.term.successors());
        out
    }
}

/// One step of an element write's path (Gate 2): a field, or an element by an index local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRef {
    Field(FieldRef),
    Index { index: LocalId, len: u32 },
}

/// A value an operation reads without taking ownership of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    /// Borrowed: read, never retained, never emptied.
    Local(LocalId),
    Const(Const),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Const {
    Int(i64),
    Bool(bool),
    Str(Vec<u8>),
    /// A number of any numeric type but `Int` (Gate 2), by its canonical bits.
    Num {
        kind: NumKind,
        bits: u64,
    },
    /// A static `Atomic`'s cell (Gate 3), by its module and name: [`Program::statics`] says
    /// what it is called and what it starts as. Immortal, as a literal's storage is.
    Cell {
        module: ModuleId,
        name: String,
    },
}

impl Const {
    #[must_use]
    pub const fn ty(&self) -> Type {
        match self {
            Self::Int(_) => Type::Int,
            Self::Bool(_) => Type::Bool,
            Self::Str(_) => Type::Str,
            Self::Num { kind, .. } => Type::of_num(*kind),
            Self::Cell { .. } => Type::Atomic,
        }
    }
}

/// A static (Gate 3): what its module's unit defines, under the name its owner spells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Static {
    pub module: ModuleId,
    pub name: String,
    /// Its durable owner, which the backend's symbol spells.
    pub owner: Owner,
    pub value: nazm_sema::StaticValue,
    /// Its `@section` (Gate 3).
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rvalue {
    /// The operand's value. For a managed type only a constant (a literal's storage is
    /// immortal); a local of managed type is read into ownership by [`Self::Copy`] or
    /// [`Self::Move`].
    Use(Operand),
    /// A new reference to what `local` holds.
    Copy(LocalId),
    /// The reference `local` holds; `local` is left empty.
    Move(LocalId),
    /// `0 - x`, checked, in the operand's type; a float's sign flips.
    Neg(Operand),
    /// In the operands' one type: checked for integers, and `/` and `%` also refuse a zero
    /// divisor; IEEE 754 for floats, which never fail.
    Arith(ArithOp, Operand, Operand),
    /// A bit operation on the left operand's integer type (Gate 2); a shift's amount is an
    /// `Int`, and one outside `0..width` fails.
    Bits(BitOp, Operand, Operand),
    /// `~x` (Gate 2). Total.
    Complement(Operand),
    /// A numeric conversion to `to` (Gate 2). An exact one to an integer type fails when the
    /// value does not fit.
    Convert {
        mode: ConvertMode,
        to: Type,
        operand: Operand,
    },
    /// Whether the operand converts to `to` exactly (Gate 2). Total.
    Fits { to: Type, operand: Operand },
    /// An array of these elements (Gate 2), copied in: plain data owns nothing.
    Array { elems: Vec<Operand> },
    /// An array of `count` copies of one value (Gate 2).
    ArrayRepeat { value: Operand, count: u32 },
    /// An element of an array, copied out; an index outside it fails (Gate 2).
    Index { base: Operand, index: Operand },
    /// Total. Both operands borrowed and of type `operand`, compared under `law`.
    Compare {
        op: CmpOp,
        law: Law,
        operand: Type,
        lhs: Operand,
        rhs: Operand,
    },
    /// The derived hash of a value of type `operand` (L6), a `UInt64`: total, the value
    /// borrowed. `docs/spec.md`, *Ordering and hashing are derived*.
    Hash { operand: Type, value: Operand },
    /// A call. The arguments are borrowed; the result is owned. The callee may have failed.
    Call { callee: FuncId, args: Vec<Operand> },
    /// A call through a function value (N50): its code is handed the closure object, then
    /// the arguments, all borrowed. The result is owned; the callee may have failed.
    CallIndirect { callee: Operand, args: Vec<Operand> },
    /// A new closure object (N50) whose code is `func`, carrying a new reference to each
    /// capture, in order. The captures are borrowed. It fails when there is no memory for it.
    Closure {
        func: FuncId,
        captures: Vec<Operand>,
    },
    /// A new reference to the `index`th thing the closure object in `env` carries (N50).
    Capture { env: LocalId, index: u32 },
    /// A runtime or compiler primitive. `elem` is the element type of a `Vec[T]` operation.
    Primitive {
        op: Intrinsic,
        elem: Option<Type>,
        args: Vec<Operand>,
    },
    /// A complete record, from moved temporaries — every field exactly once, by declaration
    /// index, in the order they were evaluated.
    Record {
        record: RecordId,
        fields: Vec<(u32, LocalId)>,
    },
    /// A complete enum value, from moved temporaries.
    Variant {
        enumeration: EnumId,
        variant: u32,
        fields: Vec<(u32, LocalId)>,
    },
    /// A field of a record, as a new reference.
    Field { base: Operand, field: FieldRef },
    /// A payload field of the variant `base` holds, as a new reference. Valid only in the arm
    /// of a `switch` on `base` for `variant`.
    Payload {
        base: LocalId,
        enumeration: EnumId,
        variant: u32,
        field: u32,
    },
}

impl Rvalue {
    /// Whether evaluating this may fail — and so whether its statement carries an edge.
    #[must_use]
    pub const fn can_fail(&self) -> bool {
        matches!(
            self,
            Self::Neg(_)
                | Self::Arith(..)
                | Self::Bits(..)
                | Self::Convert { .. }
                | Self::Index { .. }
                | Self::Call { .. }
                | Self::CallIndirect { .. }
                | Self::Closure { .. }
                | Self::Primitive { .. }
        )
    }
}

/// The concrete types a program's MIR mentions, by the checker's session-local ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Types {
    pub records: std::collections::BTreeMap<u32, RecordInfo>,
    pub enums: std::collections::BTreeMap<u32, EnumInfo>,
    /// `Vec` types, by `VecId` index: the element and the element's canonical key.
    pub vecs: std::collections::BTreeMap<u32, VecInfo>,
    /// Function types, by `FnTypeId` index (N50).
    pub fns: std::collections::BTreeMap<u32, FnInfo>,
    /// Generic channel types, by `ChanId` index (N53): the element, as a `Vec`'s is.
    pub chans: std::collections::BTreeMap<u32, VecInfo>,
    /// Array types, by `ArrayId` index (Gate 2): the element and the length.
    pub arrays: std::collections::BTreeMap<u32, ArrayInfo>,
}

/// An array type, concrete (Gate 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayInfo {
    pub elem: Type,
    pub len: u32,
    pub source: String,
}

/// A function type's signature, concrete (N50).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnInfo {
    pub params: Vec<Type>,
    pub ret: Type,
    pub source: String,
}

/// A concrete type's durable name: its definition and its arguments' canonical keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeIdentity {
    pub owner: Owner,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// The field's name: the language's canonical order is by it.
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordInfo {
    /// Display, e.g. `Box[Int]`.
    pub source: String,
    pub identity: TypeIdentity,
    /// The canonical key a digest names it by.
    pub key: String,
    /// By declaration index.
    pub fields: Vec<FieldInfo>,
    /// Declaration indices in the language's canonical order (by name).
    pub canonical: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub canonical: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumInfo {
    pub source: String,
    pub identity: TypeIdentity,
    pub key: String,
    /// By declaration index.
    pub variants: Vec<VariantInfo>,
    /// Declaration indices in canonical order (by name).
    pub canonical: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VecInfo {
    pub elem: Type,
    /// The element's canonical key.
    pub elem_key: String,
    pub source: String,
}

impl Types {
    #[must_use]
    pub fn record(&self, id: RecordId) -> Option<&RecordInfo> {
        self.records
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
    }

    #[must_use]
    pub fn enumeration(&self, id: EnumId) -> Option<&EnumInfo> {
        self.enums
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
    }

    /// An array type's element and length (Gate 2).
    #[must_use]
    pub fn array(&self, id: nazm_sema::ArrayId) -> Option<(Type, u32)> {
        self.arrays
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
            .map(|a| (a.elem, a.len))
    }

    #[must_use]
    pub fn vec(&self, id: nazm_sema::VecId) -> Option<&VecInfo> {
        self.vecs
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
    }

    #[must_use]
    pub fn function(&self, id: nazm_sema::FnTypeId) -> Option<&FnInfo> {
        self.fns
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
    }

    /// Whether two types are one up to the effects of the function types in them (N51): an
    /// effect parameter makes no instance, so a callee's parameter `fn(…) ! { E }` is what its
    /// caller passes as `fn(…) ! { io }`, and below the checker effects mean nothing.
    #[must_use]
    pub fn same_shape(&self, a: Type, b: Type) -> bool {
        if a == b {
            return true;
        }
        // A capability of any kind is one word below the checker (N79), as in
        // `nazm_sema`'s `same_shape`: an `IoCap` passed where an `OutCap` is expected is the
        // checker's attenuation, settled before Core IR. Until N104 this table lacked the rule,
        // and an attenuating call failed MIR validation.
        if let (Type::Cap(_), Type::Cap(_)) = (a, b) {
            return true;
        }
        let (Type::Fn(x), Type::Fn(y)) = (a, b) else {
            return false;
        };
        match (self.function(x), self.function(y)) {
            (Some(p), Some(q)) => {
                p.params.len() == q.params.len()
                    && p.params
                        .iter()
                        .zip(&q.params)
                        .all(|(a, b)| self.same_shape(*a, *b))
                    && self.same_shape(p.ret, q.ret)
            }
            _ => false,
        }
    }

    /// The `Vec` type whose element is `elem`, if the program has one.
    #[must_use]
    pub fn vec_of(&self, elem: Type) -> Option<Type> {
        self.vecs
            .iter()
            .find(|(_, v)| v.elem == elem)
            .map(|(id, _)| Type::Vec(nazm_sema::VecId::from_index(*id as usize)))
    }

    /// The `Chan[T]` type whose element is `elem`, if the program has one (N53).
    #[must_use]
    pub fn chan_of(&self, elem: Type) -> Option<Type> {
        self.chans
            .iter()
            .find(|(_, v)| v.elem == elem)
            .map(|(id, _)| Type::ChanOf(nazm_sema::ChanId::from_index(*id as usize)))
    }

    /// A generic channel's element (N53).
    #[must_use]
    pub fn chan(&self, id: nazm_sema::ChanId) -> Option<&VecInfo> {
        self.chans
            .get(&(u32::try_from(id.index()).unwrap_or(u32::MAX)))
    }

    /// Whether a value of `t` owns anything a drop must give back. The one classification
    /// every cleanup follows: a record or enum is managed exactly when some field it can hold
    /// is, transitively. Recursion through a type's own fields is impossible — the checker
    /// refuses a type that contains itself by value — and is bounded anyway.
    #[must_use]
    pub fn managed(&self, t: Type) -> bool {
        self.managed_within(t, 0)
    }

    fn managed_within(&self, t: Type, depth: usize) -> bool {
        if depth > nazm_sema::MAX_TYPE_DEPTH * 4 {
            return true;
        }
        match t {
            Type::Str
            | Type::Ints
            | Type::Strs
            | Type::Chan
            | Type::ChanOf(_)
            | Type::Vec(_)
            | Type::Box(_)
            | Type::List(_)
            | Type::Bytes
            | Type::Text
            | Type::Atomic
            | Type::Fn(_) => true,
            // An array holds plain data (Gate 2).
            Type::Int
            | Type::Bool
            | Type::Cap(_)
            | Type::OsHandle
            | Type::Param(_)
            | Type::Num(_)
            | Type::Array(_) => false,
            Type::Record(id) => self.record(id).is_some_and(|r| {
                r.fields
                    .iter()
                    .any(|f| self.managed_within(f.ty, depth + 1))
            }),
            Type::Enum(id) => self.enumeration(id).is_some_and(|e| {
                e.variants.iter().any(|v| {
                    v.fields
                        .iter()
                        .any(|f| self.managed_within(f.ty, depth + 1))
                })
            }),
        }
    }

    /// Whether a local of this type needs cleanup.
    #[must_use]
    pub fn managed_local(&self, t: LocalTy) -> bool {
        match t {
            LocalTy::Value(v) => self.managed(v),
            LocalTy::Tasks => true,
        }
    }

    /// A payload field's type.
    #[must_use]
    pub fn payload(&self, e: EnumId, variant: u32, field: u32) -> Option<Type> {
        self.enumeration(e)?
            .variants
            .get(variant as usize)?
            .fields
            .get(field as usize)
            .map(|f| f.ty)
    }

    /// A record field's type.
    #[must_use]
    pub fn field(&self, r: RecordId, field: u32) -> Option<Type> {
        self.record(r)?.fields.get(field as usize).map(|f| f.ty)
    }
}
