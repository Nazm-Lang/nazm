//! The representation itself.
//!
//! `docs/architecture.md` §7.41 is the law this file follows; each rule below is stated there
//! first. The short version: every value is typed by what the checker established, every name
//! is an identity, every transfer of control names what it leaves, and nothing here says how
//! the source was written or how a backend will lay it out.

use nazm_sema::{EffectSet, EnumId, FnId, Intrinsic, ModuleId, RecordId, Type};
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
}

impl Program {
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
    /// Made by this IR, not written: a `?`'s success value or error value.
    Temp(Temp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temp {
    Ok,
    Err,
}

impl Temp {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Err => "err",
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
}

impl Const {
    #[must_use]
    pub const fn ty(&self) -> Type {
        match self {
            Self::Int(_) => Type::Int,
            Self::Bool(_) => Type::Bool,
            Self::Str(_) => Type::Str,
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

/// One step of an assignment's place: a field of a record, by identity.
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
        path: Vec<Step>,
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
    /// `-x`, which fails on the one `Int` it cannot negate.
    Neg(Box<Expr>),
    /// Checked `Int` arithmetic: overflow and a zero divisor stop the program.
    Arith {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
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
    /// payload.
    Structural,
    /// A type parameter that requires equality: its instance's law.
    Parametric,
}

impl Law {
    /// The law a comparison of `operand` by `op` follows, or `None` where the language gives
    /// that type none — which checking has refused before this is asked.
    #[must_use]
    pub fn of(op: CmpOp, operand: Type) -> Option<Self> {
        match (op.is_ordering(), operand) {
            (_, Type::Int) => Some(Self::Int),
            (true, _) => None,
            (false, Type::Bool) => Some(Self::Bool),
            (false, Type::Str) => Some(Self::Bytes),
            (false, Type::Record(_) | Type::Enum(_)) => Some(Self::Structural),
            (false, Type::Param(_)) => Some(Self::Parametric),
            (
                false,
                Type::Ints | Type::Strs | Type::Chan | Type::Vec(_) | Type::Cap(_) | Type::Fn(_),
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
            _ => Ends::FallsThrough,
        }
    }
}
