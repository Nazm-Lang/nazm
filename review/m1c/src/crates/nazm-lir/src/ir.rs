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

use nazm_span::Span;

/// The value types this backend can represent. Not the language's — `Str` exists and is
/// refused by name in [`crate::lower`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
}

impl Ty {
    /// The LLVM type name.
    #[must_use]
    pub fn llvm(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Bool => "i1",
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
        }
    }
}

/// Which function. An index into [`Module::functions`], resolved once during lowering.
///
/// Calls carry this rather than a name: looking a string up again at emission time would
/// be a second name resolution, and two resolutions are two chances to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncId(pub usize);

impl FuncId {
    /// The LLVM symbol for this function.
    ///
    /// Generated from the index and **never from the source name**. A Nazm identifier
    /// would need escaping rules to be safe in an LLVM identifier, and the escaping is
    /// the bug — so there is none to get wrong. It also means no program can collide
    /// with the runtime helper, with the platform entry point, or with `printf`,
    /// `write` or `exit`.
    #[must_use]
    pub fn symbol(self) -> String {
        format!("@nz.f{}", self.0)
    }
}

/// One compiled program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Indexed by [`FuncId`].
    pub functions: Vec<Function>,
    /// Nazm's `main` — an ordinary function here. The C entry point is a wrapper the
    /// emitter adds, which is what keeps a Nazm program from colliding with `@main`.
    pub entry: FuncId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub id: FuncId,
    /// The source name. For comments and diagnostics only — never for a symbol, and
    /// never resolved against.
    pub name: String,
    /// Slot `i` is `locals[i]`, and the **first `params` slots are the parameters**, in
    /// order. Slots are unique within a function even where names are not: a branch-local
    /// binding and an outer one that shadows it get different slots, so the emitter never
    /// has to know about scopes.
    ///
    /// Each function owns its own slot space, so a parameter, a caller's local and a
    /// callee's local that share a spelling cannot share storage.
    pub locals: Vec<Local>,
    pub params: usize,
    pub ret: Ty,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    /// Kept for `--emit-ir` to be readable by a person. Never used to resolve anything.
    pub name: String,
    pub ty: Ty,
}

/// Statements, then the value the block produces.
///
/// The same shape for a function body and for an `if` branch, because they are the same
/// thing in the language: `docs/spec.md` has one `Block`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub result: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Bind `value` to a slot. Immutable: M1b has no assignment, so a slot is written
    /// exactly once and the emitter needs no notion of reassignment.
    Let { slot: usize, value: Expr },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Load {
        slot: usize,
        ty: Ty,
    },
    /// Fails on `Int::MIN`, which is the only negation without a representable result.
    Neg {
        operand: Box<Expr>,
        span: Span,
    },
    /// `Int, Int -> Int`. Every one of these can fail.
    Arith {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// `T, T -> Bool`. None of these can fail.
    Compare {
        op: CmpOp,
        /// The **operand** type, not the result — the result is always `Bool`. The
        /// emitter needs this to pick the comparison width.
        operand_ty: Ty,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A direct call to a known function.
    ///
    /// There is no indirect call, because the language has no function values. `ret` is
    /// carried so [`Expr::ty`] stays total without consulting the module.
    Call {
        callee: FuncId,
        args: Vec<Expr>,
        ret: Ty,
    },
    /// Value-position `if`. Both branches are present and produce `ty`.
    ///
    /// There is no statement-position variant, and that is the point: this backend
    /// compiles the `if` that produces a value and refuses the one that does not, rather
    /// than having one node whose meaning depends on where it sits.
    If {
        cond: Box<Expr>,
        then_block: Box<Block>,
        else_block: Box<Block>,
        ty: Ty,
    },
}

impl Expr {
    /// What this expression produces.
    ///
    /// Total, and derived from what the node recorded rather than from context — the
    /// emitter calls it to size a `phi` and to pick a comparison, and a wrong answer
    /// there is a miscompile rather than a diagnostic.
    #[must_use]
    pub fn ty(&self) -> Ty {
        match self {
            Self::Int(_) | Self::Neg { .. } | Self::Arith { .. } => Ty::Int,
            Self::Bool(_) | Self::Compare { .. } => Ty::Bool,
            Self::Load { ty, .. } | Self::If { ty, .. } => *ty,
            Self::Call { ret, .. } => *ret,
        }
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
