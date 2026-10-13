//! The representation itself.
//!
//! `docs/architecture.md` §7.41 is the law this file follows; each rule below is stated there
//! first. The short version: every value is typed by what the checker established, every name
//! is an identity, every transfer of control names what it leaves, and nothing here says how
//! the source was written or how a backend will lay it out.

use nazm_sema::traits::TraitId;
use nazm_sema::{
    ConvertMode, EffectSet, EnumId, FnId, Intrinsic, ModuleId, NumKind, RecordId, Type,
};
use nazm_span::Span;

/// A checked compilation's Core IR.
///
/// One [`Function`] per definition the checker resolved, **at the definition's own index**:
/// `functions[i].def` is `FnId` *i*, which [`crate::verify`] holds. Session-local, like the
/// resolution it was built from; the printed form ([`crate::print`]) and the digests
/// ([`crate::digest`]) name everything durably.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
    /// The root module's `main`, where a program has one. A module checked as a library has
    /// none, and its Core IR is the same otherwise.
    pub entry: Option<FnId>,
    /// The checker's impl table (N78): for each trait and type with an impl, the function
    /// implementing each of the trait's methods. The one table a [`ExprKind::CallTrait`] is
    /// resolved in — per instance by MIR, per frame by the interpreter.
    pub impls: Vec<ImplRow>,
    /// Each generic record and enum instance, with the definition it instantiates (R1b, L11b):
    /// an impl of a generic type is filed under the definition, and a lookup for an instance
    /// falls back to it. Sorted by the instance.
    pub templates: Vec<(Type, Type)>,
    /// Every static of the program (Gate 3), as the checker recorded them: storage each
    /// module's unit defines.
    pub statics: Vec<nazm_sema::StaticDef>,
    /// The canonical order of every record's fields and every enum's variants and payloads
    /// (L6): what a derived order and a derived hash walk, so that reordering a declaration —
    /// an edit the language says means nothing — changes no comparison and no hash.
    pub orders: Orders,
    /// `@std/hash`'s `Hash`, when the program has it (L6): a call through it whose receiver's
    /// type has no impl is that type's derived hash or `same`.
    pub derived_hash: Option<DerivedHash>,
}

/// The derivable trait (L6) and the positions of its two methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedHash {
    pub trait_id: TraitId,
    /// `hash(self) -> UInt64`.
    pub hash: u32,
    /// `same(self, other) -> Bool`, which a derived type answers with `==`.
    pub same: u32,
}

/// Canonical orders (L6), by definition index: declaration indices sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Orders {
    /// By [`nazm_sema::RecordId`] index: the record's fields.
    pub records: Vec<Vec<u32>>,
    /// By [`nazm_sema::EnumId`] index.
    pub enums: Vec<EnumOrder>,
}

/// One enum's canonical orders (L6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumOrder {
    /// Its variants, as declaration indices sorted by name.
    pub variants: Vec<u32>,
    /// By declaration index: that variant's payload fields, sorted by name.
    pub payloads: Vec<Vec<u32>>,
    /// By declaration index: that variant's name, which a derived hash starts from.
    pub names: Vec<String>,
}

/// One impl, as Core IR resolves calls through a bound against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplRow {
    pub trait_id: TraitId,
    pub self_ty: Type,
    /// By the trait's method order.
    pub methods: Vec<FnId>,
}

impl Program {
    /// The function implementing method `method` of `trait_id` for `ty`, from the impl table.
    #[must_use]
    pub fn method_fn(&self, trait_id: TraitId, ty: Type, method: u32) -> Option<FnId> {
        let row = |t: Type| {
            self.impls
                .iter()
                .find(|r| r.trait_id == trait_id && r.self_ty == t)
        };
        // An instance of a generic type finds the impl of its definition (L11b).
        let template = || {
            self.templates
                .binary_search_by(|(i, _)| i.cmp(&ty))
                .ok()
                .and_then(|at| self.templates.get(at))
                .and_then(|(_, t)| row(*t))
        };
        row(ty)
            .or_else(template)
            .and_then(|r| r.methods.get(method as usize).copied())
    }

    /// The function `id` names.
    #[must_use]
    pub fn function(&self, id: FnId) -> Option<&Function> {
        self.functions.get(id.index()).filter(|f| f.def == id)
    }

    /// The functions `module` declares, in declaration order.
    pub fn of_module(&self, module: ModuleId) -> impl Iterator<Item = &Function> {
        self.functions.iter().filter(move |f| f.module == module)
    }
}

/// One function: its signature, its contract, its locals and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The definition this is the body of — its identity. Never its name.
    pub def: FnId,
    pub module: ModuleId,
    /// Display only: what the source called it. Nothing resolves against it.
    pub name: String,
    /// How many type parameters it declares. A generic function's types mention them, and
    /// each call records its arguments; see *Generics* in §7.41.
    pub type_params: u32,
    /// How many of [`Self::locals`] are parameters. They come first.
    pub params: u32,
    /// Every local, by [`LocalId`]: the checker's slots in its order, then this IR's own
    /// temporaries.
    pub locals: Vec<Local>,
    pub ret: Type,
    /// Every literal the body uses, by [`ConstId`], each once, in first-use order.
    pub consts: Vec<Const>,
    /// How many loops the body has. [`LoopId`]s are `0..loops`, in source order.
    pub loops: u32,
    pub effects: Effects,
    /// A value region: its tail is the function's result where the statements fall through.
    pub body: Block,
    /// The declaration, its name, and its written return type — for diagnostics.
    pub span: Span,
    pub name_span: Span,
    pub ret_span: Span,
    /// The C symbol of a foreign declaration (N42): a function whose body is not Nazm's.
    /// Its `body` is an empty region no consumer walks.
    pub foreign: Option<String>,
    /// The section `@section` places it in (Gate 3): a board's image only.
    pub section: Option<String>,
    /// The interrupt `@interrupt` makes it the handler of (Gate 3): a board's image only.
    pub interrupt: Option<String>,
    /// Whether `@on_failure` makes it the board's failure hook (Gate 3).
    pub on_failure: bool,
    /// `@task`'s priority and period (Gate 3): a board's periodic task.
    pub task: Option<(u32, u64)>,
    /// The C symbol an exported function is also callable as (N55): a Nazm body with a C entry.
    pub export: Option<String>,
    /// For a closure's body (N50): the locals that hold its captures, in capture order. They
    /// are filled from the closure when it is called; empty for every other function.
    pub captures: Vec<LocalId>,
    /// Whether this is a closure's body (N50): its local 0, a parameter, is the closure
    /// itself, and its other parameters follow.
    pub closure: bool,
}

impl Function {
    /// The parameters' locals.
    #[must_use]
    pub fn param_locals(&self) -> &[Local] {
        self.locals.get(..self.params as usize).unwrap_or(&[])
    }

    /// The capabilities it is handed: the kind of each capability-typed parameter. Authority
    /// is a value the function holds, never a flag it carries (N37).
    #[must_use]
    pub fn authority(&self) -> nazm_sema::CapabilitySet {
        self.param_locals()
            .iter()
            .filter_map(|l| match l.ty {
                Type::Cap(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    #[must_use]
    pub fn local(&self, id: LocalId) -> Option<&Local> {
        self.locals.get(id.index())
    }

    #[must_use]
    pub fn constant(&self, id: ConstId) -> Option<&Const> {
        self.consts.get(id.index())
    }
}

/// A function's effect contract, as the checker settled it (N36). Read, never re-inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effects {
    /// The set its declaration writes (`! { … }`), or `None` where it writes none.
    pub declared: Option<EffectSet>,
    /// What its body requires: the checker's least fixed point over the calls it makes.
    pub required: EffectSet,
}

impl Effects {
    /// What a caller is held to: the declared set where there is one, and otherwise what the
    /// body requires.
    #[must_use]
    pub fn published(self) -> EffectSet {
        self.declared.unwrap_or(self.required)
    }

    /// Whether calling it can have no effect a caller must declare.
    #[must_use]
    pub fn is_pure(self) -> bool {
        self.published().is_empty()
    }
}

/// A local of this compilation, by position in its function. Parameters are the first ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

impl LocalId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A literal of one function, by position in its constant table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstId(pub u32);

impl ConstId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A loop of one function, in source order. What a `break` or `continue` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoopId(pub u32);

impl LoopId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub ty: Type,
    /// Display only. Two locals may share one, and a digest never reads it.
    pub name: String,
    pub origin: LocalOrigin,
    /// Where it was declared, or — for a temporary — the source it was made for.
    pub at: Span,
}

/// Where a local came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalOrigin {
    Param,
    /// A `let`, a `let mut`, or a pattern's binding. Mutability is a checking fact — the
    /// checker already refused every assignment it forbids — and is kept for a reader.
    Binding {
        mutable: bool,
    },
    /// Made by this IR, not written: a `?`'s success value or error value, or what C
    /// returned (N85).
    Temp(Temp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temp {
    Ok,
    Err,
    /// What a foreign function returned, before it is a handle, a string or `None` (N85).
    Foreign,
    /// The value a function returns, held while its `ensures` clauses are checked (N87).
    Result,
    /// The value `try_convert` tests before converting it (Gate 2).
    Convert,
    /// What a `for` loop iterates, evaluated once (R1a).
    ForSeq,
    /// A `for` loop's step count, or its range's current value (R1a).
    ForIndex,
    /// A `for` range's end, evaluated once (R1a).
    ForEnd,
    /// The `Box` or `List` `box_new` or `list_from_vec` is making, before anything else can see
    /// it (R1a).
    Made,
    /// A general `match`'s scrutinee, evaluated once (R1a).
    Scrutinee,
    /// Which arm of a general `match` runs (R1a).
    Arm,
    /// A payload field a general `match`'s test or extraction reads (R1a).
    Payload,
}

impl Temp {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Err => "err",
            Self::Foreign => "c",
            Self::Result => "result",
            Self::Convert => "convert",
            Self::ForSeq => "seq",
            Self::ForIndex => "i",
            Self::ForEnd => "end",
            Self::Made => "made",
            Self::Scrutinee => "scrutinee",
            Self::Arm => "arm",
            Self::Payload => "payload",
        }
    }
}

/// A literal, canonically: the types the language has literals for, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Const {
    Int(i64),
    Bool(bool),
    /// Bytes: `docs/spec.md` makes a `Str` a byte sequence.
    Str(Vec<u8>),
    /// A value of a numeric type other than `Int` (Gate 2), as its canonical bits
    /// ([`nazm_sema::numeric::NumVal`]).
    Num {
        kind: NumKind,
        bits: u128,
    },
    /// A static `Atomic`'s cell (Gate 3), by its module and name: every use is the one cell
    /// the module's [`Program::statics`] entry starts.
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

/// How a statement sequence finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    /// Reaches its end; whatever follows runs.
    FallsThrough,
    /// Every path leaves through `return`, `break` or `continue`.
    Transfers,
}

/// A region: statements, perhaps a tail, and the locals it owns.
///
/// Two kinds, told apart by where they appear rather than by a flag. A **value** region — a
/// function body, a value `if` branch, a value `match` arm, a block used as a value — has a
/// tail exactly when its statements fall through. A **statement** region — a loop body, a
/// statement `if` branch or `match` arm, a `scope` body, a block written as a statement — has
/// none. [`crate::verify`] holds both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    /// How [`Self::stmts`] finish. A tail may itself never produce a value — its type says so
    /// — and the statements still fall through to it.
    pub ends: Ends,
    /// The locals bound directly in this region, whose values the language releases at its
    /// exit — the semantic ownership fact a MIR lowers from. See *Memory management* in §7.41.
    pub owns: Vec<LocalId>,
    pub at: Span,
}

impl Block {
    /// The type the region produces, or `None` where it never produces one.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        self.tail.as_ref().and_then(|t| t.ty)
    }
}

/// One step of an assignment's place: a field of a record, or an element of an array
/// (Gate 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    Field(Step),
    /// `[index]` into an array of `len` elements; out of range stops the program (`N0405`).
    Index {
        index: Expr,
        len: u32,
        at: Span,
    },
}

/// A field step of an assignment's place: a field of a record, by identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub record: RecordId,
    /// The field's declaration index in `record`.
    pub field: u32,
    pub at: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Bind a local. `let` and `let mut` are one statement; see [`LocalOrigin`].
    Let {
        local: LocalId,
        value: Expr,
    },
    /// Write a local, or a field path inside one. The value is evaluated first.
    Assign {
        local: LocalId,
        path: Vec<PathStep>,
        value: Expr,
        at: Span,
    },
    /// Evaluate for effect and discard the value — the language's `e;`.
    Eval(Expr),
    /// A statement `if`: its branches are statement regions, and without `else` it always
    /// falls through.
    If {
        cond: Expr,
        then: Block,
        otherwise: Option<Block>,
        at: Span,
    },
    /// A statement `match`: its arms' bodies are statement regions.
    Match(Box<Match>),
    /// `while`: the condition is evaluated before each turn, outside this loop — a `break`
    /// in it names the enclosing loop.
    Loop {
        id: LoopId,
        cond: Expr,
        body: Block,
        at: Span,
    },
    /// A block written as a statement: a region of its own.
    Block(Block),
    /// `scope { … }`: every task started in it is joined at its exit, before any transfer
    /// out of it proceeds.
    Scope {
        body: Block,
        at: Span,
    },
    /// `spawn f(args);`, which starts a task running a function of this compilation.
    Spawn {
        callee: FnId,
        args: Vec<Expr>,
        at: Span,
        callee_at: Span,
    },
    Return {
        value: Expr,
        at: Span,
    },
    Break {
        target: LoopId,
        at: Span,
    },
    Continue {
        target: LoopId,
        at: Span,
    },
}

/// A value, with the type it has when it completes and the span a diagnostic about it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    /// `None` exactly when it never completes with a value: an `if`, `match` or block every
    /// path of which transfers control. Everything else has the type the checker gave it.
    pub ty: Option<Type>,
    /// Where it is. For an operator, the operator's; for a call, the whole call's; for a
    /// projection, the field name's.
    pub at: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Const(ConstId),
    Local(LocalId),
    /// `-x`, which fails on the one `Int` it cannot negate — and, at any other integer width,
    /// on its minimum, or on an unsigned value other than zero. A float's sign flips.
    Neg(Box<Expr>),
    /// Checked arithmetic on one numeric type, the expression's own (Gate 2 for every type but
    /// `Int`): for integers, overflow and a zero divisor stop the program; floats never do.
    Arith {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A bit operation on one integer type (Gate 2): for a shift, `rhs` is the amount, an
    /// `Int`, and an amount out of `0..width` stops the program (`N0408`).
    Bits {
        op: BitOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `~x` (Gate 2).
    Complement(Box<Expr>),
    /// A numeric conversion to the expression's type (Gate 2): exact, wrapping or saturating.
    /// `try_convert` is a [`Self::Fits`] test around an exact one.
    Convert {
        mode: ConvertMode,
        operand: Box<Expr>,
    },
    /// Whether the operand converts to `to` exactly (Gate 2): a `Bool`.
    Fits {
        to: Type,
        operand: Box<Expr>,
    },
    /// An array of the elements, in the order written, which is evaluation order (Gate 2).
    Array {
        elems: Vec<Expr>,
    },
    /// An array of `count` copies of `value`, which is evaluated once (Gate 2).
    ArrayRepeat {
        value: Box<Expr>,
        count: u32,
    },
    /// An element of an array: the base, then the index, evaluated in that order; an index
    /// outside the array stops the program (`N0405`) (Gate 2).
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// A comparison, with the law it compares by already settled.
    Compare {
        op: CmpOp,
        operand: Type,
        law: Law,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A call to a function of this compilation.
    Call {
        callee: FnId,
        /// The callee's type arguments, in its parameters' order, as this body sees them.
        /// Empty for a callee that is not generic.
        type_args: Vec<Type>,
        args: Vec<Expr>,
        callee_at: Span,
    },
    /// A built-in, resolved: a runtime primitive by identity.
    Builtin {
        op: Intrinsic,
        /// A `Vec[T]` operation's element type; empty for every other built-in.
        type_args: Vec<Type>,
        args: Vec<Expr>,
        callee_at: Span,
    },
    /// Construct a record. Initialisers in the order written, which is evaluation order.
    Record {
        record: RecordId,
        /// How many fields it has; every one is initialised exactly once.
        arity: u32,
        fields: Vec<Init>,
    },
    /// Construct an enum value. Initialisers in the order written.
    Variant {
        enumeration: EnumId,
        variant: u32,
        arity: u32,
        fields: Vec<Init>,
    },
    /// Read a field of a record value.
    Field {
        base: Box<Expr>,
        record: RecordId,
        field: u32,
    },
    Match(Box<Match>),
    /// A value `if`: both branches are value regions.
    If {
        cond: Box<Expr>,
        then: Block,
        otherwise: Block,
    },
    /// A block used as a value.
    Block(Block),
    /// A closure (N50): the function its body was lifted to, and the enclosing body's locals
    /// it copies, in the order [`Function::captures`] receives them.
    Closure {
        func: FnId,
        captures: Vec<LocalId>,
    },
    /// A named function as a value (N50).
    FnValue(FnId),
    /// A call through a function value (N50): the callee is a value of function type.
    CallIndirect {
        callee: Box<Expr>,
        args: Vec<Expr>,
        callee_at: Span,
    },
    /// A method call through a type parameter's bound (N78): method `method` of `trait_id`
    /// on a receiver of type `self_ty`, which mentions the calling function's own type
    /// parameters. Which function runs is the impl table's answer for `self_ty` once those
    /// are known — per instance in MIR, which never contains this, and per frame in the
    /// interpreter. The receiver is `args[0]`. A call on a concrete type is a [`Self::Call`].
    CallTrait {
        trait_id: TraitId,
        method: u32,
        self_ty: Type,
        args: Vec<Expr>,
        callee_at: Span,
    },
}

/// One initialiser: a field or payload field by declaration index, and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Init {
    pub field: u32,
    pub value: Expr,
    pub at: Span,
}

/// `match`, in either position.
///
/// The scrutinee is evaluated once, into a value the `match` owns; the arm whose variant it
/// is runs with its bindings bound. Exhaustive and disjoint by checking, so the arm order —
/// kept as written — has no meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub scrutinee: Expr,
    pub enumeration: EnumId,
    pub arms: Vec<Arm>,
    pub origin: MatchOrigin,
    pub at: Span,
}

/// Whether a `match` was written, or is what a `?` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOrigin {
    Written,
    /// `operand?`: an `Ok` arm whose value is the `match`'s, and an `Err` arm that returns the
    /// enclosing function's `Result` with the same error. The source marker for a
    /// compiler-made node; the arms' spans are the operator's.
    Propagate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    /// The variant's declaration index in the `match`'s enum.
    pub variant: u32,
    /// Payload fields bound to locals, in the order written. A discarded field binds none.
    pub bindings: Vec<Binding>,
    /// A value region in a value `match`, a statement region in a statement one.
    pub body: Block,
    pub at: Span,
}

/// A payload field bound to a local, for the arm's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub field: u32,
    pub local: LocalId,
    pub at: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl ArithOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }
}

/// A bit operator (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitOp {
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

impl BitOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::And => "&",
            Self::Or => "|",
            Self::Xor => "^",
            Self::Shl => "<<",
            Self::Shr => ">>",
        }
    }

    /// The shared semantics' operator.
    #[must_use]
    pub const fn numeric(self) -> nazm_sema::numeric::Bit {
        use nazm_sema::numeric::Bit;
        match self {
            Self::And => Bit::And,
            Self::Or => Bit::Or,
            Self::Xor => Bit::Xor,
            Self::Shl => Bit::Shl,
            Self::Shr => Bit::Shr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl CmpOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
        }
    }

    #[must_use]
    pub const fn is_ordering(self) -> bool {
        !matches!(self, Self::Eq | Self::NotEq)
    }
}

/// Which law a comparison compares by (N13, N14). Settled here, so no backend discovers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Law {
    /// `<`, `<=`, `>`, `>=` on `Int`, and `==` on `Int`.
    Int,
    Bool,
    /// `Str`: equal bytes, whatever backs them.
    Bytes,
    /// A record's or enum's derived equality: field by field, or the active variant and its
    /// payload. For an ordering (L6), its derived order: fields, and variants, in the byte
    /// order of their names.
    Structural,
    /// A type parameter that requires equality: its instance's law.
    Parametric,
    /// A numeric type other than `Int` (Gate 2): its kind's order and equality — unsigned for
    /// an unsigned integer, IEEE 754 for a float.
    Num,
}

impl Law {
    /// The law a comparison of `operand` by `op` follows, or `None` where the language gives
    /// that type none — which checking has refused before this is asked.
    #[must_use]
    pub fn of(op: CmpOp, operand: Type) -> Option<Self> {
        match (op.is_ordering(), operand) {
            (_, Type::Int) => Some(Self::Int),
            (_, Type::Num(_)) => Some(Self::Num),
            // Ordering is derived for `Bool`, `Str` and records and enums too (L6).
            (_, Type::Bool) => Some(Self::Bool),
            (_, Type::Str) => Some(Self::Bytes),
            (_, Type::Record(_) | Type::Enum(_)) => Some(Self::Structural),
            (true, _) => None,
            // An array compares element by element, by its element's law (Gate 2).
            (false, Type::Array(_)) => Some(Self::Structural),
            (false, Type::Text) => Some(Self::Bytes),
            // A `Box` or `List` compares by what it holds (L6).
            (false, Type::Box(_) | Type::List(_)) => Some(Self::Structural),
            (false, Type::Param(_)) => Some(Self::Parametric),
            (
                false,
                Type::Ints
                | Type::Strs
                | Type::Chan
                | Type::ChanOf(_)
                | Type::Vec(_)
                | Type::Cell(_)
                | Type::Bytes
                | Type::Cap(_)
                | Type::OsHandle
                | Type::Atomic
                | Type::Fn(_),
            ) => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Bytes => "bytes",
            Self::Structural => "structural",
            Self::Parametric => "parametric",
            Self::Num => "num",
        }
    }
}

impl Stmt {
    /// How this statement finishes: the rule every consumer shares, stated once.
    ///
    /// A statement `if` transfers only with an `else` and both branches transferring; a
    /// statement `match` only when every arm does, or when its scrutinee never produces a
    /// value; a `scope` and a block as their bodies do. An expression evaluated for effect
    /// falls through here even if it cannot complete — its own flow leaves at run time,
    /// and no statement after it is lowered differently for it.
    #[must_use]
    pub fn ends(&self) -> Ends {
        match self {
            Self::Return { .. } | Self::Break { .. } | Self::Continue { .. } => Ends::Transfers,
            Self::If {
                then,
                otherwise: Some(otherwise),
                ..
            } if then.ends == Ends::Transfers && otherwise.ends == Ends::Transfers => {
                Ends::Transfers
            }
            Self::Match(m)
                if m.scrutinee.ty.is_none()
                    || (!m.arms.is_empty()
                        && m.arms.iter().all(|a| a.body.ends == Ends::Transfers)) =>
            {
                Ends::Transfers
            }
            Self::Scope { body, .. } | Self::Block(body) => body.ends,
            // `exit_with` ends the process (R1a uses it where exhaustiveness makes a branch
            // impossible, so the branch needs no value).
            Self::Eval(Expr {
                kind:
                    ExprKind::Builtin {
                        op: nazm_sema::Intrinsic::ExitWith,
                        ..
                    },
                ..
            }) => Ends::Transfers,
            _ => Ends::FallsThrough,
        }
    }
}
