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
///
/// A block used where no value is wanted — a loop body, a branch of a statement-position
/// `if` — is not this type. It is a plain `Vec<Stmt>`, so a context that cannot use a
/// value cannot be handed one by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The tail expression, or `None` when **every path through `stmts` transfers
    /// control** and the block therefore has no tail and no value.
    ///
    /// `None` is not "no value yet" and never licenses inventing one: it is the
    /// `Completion::Diverges` the checker reports for the same block. Anything written
    /// after the transfer is already `N0313`, so there is no tail to lose.
    ///
    /// The converse does not hold, and that is deliberate: `Some(_)` does not promise a
    /// value will be produced. A statement whose *initialiser* transfers
    /// (`let x = if c { break; } else { continue; };`) leaves the checker — and so this
    /// field — saying the block falls through, while emission knows better and stops.
    /// Emission is allowed to be more precise than the checker; it must never be less.
    pub result: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Bind `value` to a slot.
    ///
    /// **Executed every time it is reached**, which is what a declaration inside a loop
    /// body means: the slot is storage, reused each iteration, and this statement is what
    /// puts the initial value back into it. Storage reuse must never become value reuse.
    Let { slot: usize, value: Expr },
    /// Store `value` into an already-bound slot.
    ///
    /// The same store as [`Stmt::Let`] — [`crate::lower`] has already resolved the name to
    /// a slot, so nothing here distinguishes them mechanically, and the emitter routes
    /// both through one path so the two cannot drift. They are separate variants because
    /// they are separate *source* constructs: `--emit-ir` names which one produced a
    /// store, and a reader who cannot tell a declaration from a reassignment cannot check
    /// the shadowing.
    ///
    /// Whether the binding was declared `let mut` is not asked here. The checker rejects
    /// assignment to an immutable binding (`N0310`) before lowering runs, and re-deciding
    /// it here would be a second mutability rule to keep in agreement.
    Assign { slot: usize, value: Expr },
    /// `while cond { body }`.
    ///
    /// The condition is an expression evaluated **on every iteration**, including the
    /// first and including after the body has changed what it reads. The body is a
    /// statement sequence with no value: a loop produces nothing, so there is no tail
    /// here and no type to carry.
    ///
    /// The condition is **not** inside this loop for the purposes of [`Stmt::Break`] and
    /// [`Stmt::Continue`]: a transfer raised while evaluating it targets the *enclosing*
    /// loop. That is the existing language's rule, read out of both implementations —
    /// `check.rs` raises `loop_depth` around the body alone, and `eval.rs` lets a
    /// `Flow::Break` from the condition return straight out of `while_loop`. See
    /// `docs/roadmap.md`, *Where a loop target binds*.
    While { cond: Expr, body: Vec<Stmt> },
    /// Statement-position `if`. Produces no value; either branch may transfer control.
    ///
    /// The same source `if` as [`Expr::If`] — the language has one, and which of these it
    /// becomes is decided by the position it was found in, exactly as checking decides it.
    /// What differs is completion, and that is what these two types record.
    ///
    /// `else_body` is `None` for an `if` with no `else`. Such a statement **always has a
    /// fall-through path**, whatever the branch does, because the condition may be false.
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    /// An expression evaluated for its effects, its value discarded.
    ///
    /// Where the language permits a tail in a position that wants no value: the end of a
    /// branch of a statement-position `if`. `nazm run` evaluates it and drops the value,
    /// so this does too — what survives is a runtime failure or a control transfer.
    Discard(Expr),
    /// Leave the enclosing **function** with this value.
    ///
    /// Not a loop transfer: a `return` inside a loop leaves the function, which is why it
    /// needs no target from the loop stack and emits its own `ret`. See `docs/roadmap.md`,
    /// *One `ret` per `return`, not a shared exit block*.
    Return(Expr),
    /// Leave the innermost enclosing loop.
    Break,
    /// Start the innermost enclosing loop's next iteration, **re-evaluating its
    /// condition**. Not a jump to the body.
    Continue,
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
        /// `None` when **neither** branch produces a value, because both transfer
        /// control. The expression is still emitted — the condition runs and the
        /// transfers happen — but it never yields anything, and no `phi` is written.
        ty: Option<Ty>,
    },
}

impl Expr {
    /// What this expression produces, or `None` when it never produces anything because
    /// every path through it transfers control.
    ///
    /// Derived from what the node recorded rather than from context. `None` is the
    /// checker's `Completion::Diverges`, and [`fits`] is the rule the checker applies to
    /// it: a diverging expression stands in for whatever type the context wanted, because
    /// control never reaches the point where its value would be used.
    #[must_use]
    pub fn ty(&self) -> Option<Ty> {
        match self {
            Self::Int(_) | Self::Neg { .. } | Self::Arith { .. } => Some(Ty::Int),
            Self::Bool(_) | Self::Compare { .. } => Some(Ty::Bool),
            Self::Load { ty, .. } => Some(*ty),
            Self::Call { ret, .. } => Some(*ret),
            Self::If { ty, .. } => *ty,
        }
    }
}

/// Whether `e` may be used where a `want` is required.
///
/// Mirrors `nazm_core`'s `value_type`, which returns the *expected* type for a
/// `Completion::Diverges` rather than an error. An expression that always transfers has
/// no type to disagree with, so it fits everywhere — and refusing it here would reject
/// programs the language accepts.
#[must_use]
pub fn fits(e: &Expr, want: Ty) -> bool {
    e.ty().is_none_or(|t| t == want)
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
