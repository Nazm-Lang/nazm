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
    /// An immutable byte sequence, as `{ pointer, byte length }` by value.
    ///
    /// Not NUL-terminated: `docs/spec.md` makes an embedded zero an ordinary byte, so
    /// the length is the length. The pointer is always valid to read `length` bytes
    /// from, and for an empty string it still points at real storage — which is what
    /// lets `memcmp` be called without a special case.
    Str,
    /// A growable sequence of `Int`: a **pointer to a header**, `{ len, cap, data }`.
    ///
    /// The handle is the header's address and never changes, which is what gives these
    /// reference semantics: growth reallocates `data` and rewrites the header in place,
    /// so every holder of the handle sees it. See `docs/spec.md`, *Sequences are shared,
    /// mutable, and never freed*.
    Ints,
    /// The same, with 16-byte `{ ptr, i64 }` elements.
    Strs,
    /// A bounded queue of `Int`, as a **pointer to a runtime header**.
    ///
    /// Reference semantics like a sequence, and for a stronger reason: a channel exists
    /// to be shared between tasks, and every one of its operations is a
    /// synchronisation point. See `docs/spec.md`, *Structured concurrency*.
    Chan,
}

impl Ty {
    /// Bytes one element of this sequence occupies, for indexing and growth.
    #[must_use]
    pub fn element_size(self) -> Option<u32> {
        match self {
            Self::Ints => Some(8),
            Self::Strs => Some(16),
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
    /// The LLVM type name.
    #[must_use]
    pub fn llvm(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Bool => "i1",
            Self::Str => "{ ptr, i64 }",
            Self::Ints | Self::Strs | Self::Chan => "ptr",
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
            Self::Bool => "false",
            Self::Str => "zeroinitializer",
            Self::Ints | Self::Strs | Self::Chan => "null",
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Str => "Str",
            Self::Ints => "Ints",
            Self::Strs => "Strs",
            Self::Chan => "Chan",
        }
    }
}

/// Which function. An index into [`Module::functions`], resolved once during lowering.
///
/// Calls carry this rather than a name: looking a string up again at emission time would
/// be a second name resolution, and two resolutions are two chances to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncId(pub usize);

impl Ty {
    /// The backend's representation of a front-end [`Type`](nazm_syntax::Type).
    ///
    /// One-to-one today, and written as a total match rather than a default arm so that
    /// adding a type to the language is a compile error here instead of a silent `Int`.
    #[must_use]
    pub const fn of(t: nazm_syntax::Type) -> Self {
        match t {
            nazm_syntax::Type::Int => Self::Int,
            nazm_syntax::Type::Bool => Self::Bool,
            nazm_syntax::Type::Str => Self::Str,
            nazm_syntax::Type::Ints => Self::Ints,
            nazm_syntax::Type::Strs => Self::Strs,
            nazm_syntax::Type::Chan => Self::Chan,
        }
    }
}

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
    /// `docs/spec.md`, *Where a loop target binds*.
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
    /// needs no target from the loop stack and emits its own `ret`. One `ret` per
    /// `return` rather than a shared exit block: nothing needs cleanup on the way out yet,
    /// and a shared block would be machinery for a guarantee this backend does not make.
    Return(Expr),
    /// Leave the innermost enclosing loop.
    Break,
    /// Start the innermost enclosing loop's next iteration, **re-evaluating its
    /// condition**. Not a jump to the body.
    Continue,
    /// `scope { body }` — the body, and then a wait for every task started inside it.
    ///
    /// The wait is not a statement of its own, because it must happen on **every** way
    /// out of the body: falling off the end, a `return`, a `break` or a `continue` that
    /// leaves the scope. Making it part of this variant is what stops emission putting
    /// it on only the first of those.
    ///
    /// `span` is carried because entering a scope allocates the list of tasks to join,
    /// and an allocation that fails is reported where the `scope` is written.
    Scope { body: Vec<Stmt>, span: Span },
    /// `spawn f(args);` — start a task, to be joined by the enclosing [`Stmt::Scope`].
    ///
    /// The callee is a [`FuncId`], not an expression: the language has no function
    /// values, so a task is always a named function.
    ///
    /// `span` is carried because starting a task can fail — the machine may refuse
    /// another thread — and that failure is reported where the `spawn` is written.
    Spawn {
        callee: FuncId,
        args: Vec<Expr>,
        /// The callee's return type, carried for the same reason [`Expr::Call`] carries
        /// it: emission needs it to write the call, and looking it up in the module
        /// again would be a second answer to a settled question. The value itself is
        /// discarded — a task's result travels through a channel or not at all.
        ret: Ty,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    /// A string literal, as its **bytes**. The source is UTF-8 so a literal's bytes are
    /// valid UTF-8; nothing downstream may assume that of every `Str`.
    Str(Vec<u8>),
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
    /// A built-in operation.
    ///
    /// Separate from [`Expr::Call`] because it resolves to emitted instructions rather
    /// than to a `FuncId`, and because **the set this backend lowers is its own**. A
    /// built-in the front end has and this does not is an ordinary backend limitation,
    /// reported as `N0101` like any other, and `lower` refuses it by name rather than
    /// emitting something approximate.
    Builtin {
        op: Builtin,
        args: Vec<Expr>,
        span: Span,
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
            Self::Str(_) => Some(Ty::Str),
            Self::Builtin { op, .. } => Some(op.lowered_ret()),
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
/// Identity, name and signature are `nazm_syntax::Intrinsic`'s. What stays here is how a
/// built-in is *emitted*, which is genuinely backend-specific — see [`Lowering`].
pub use nazm_syntax::Intrinsic as Builtin;

/// What lowering needs to know about a built-in, over and above its identity.
///
/// An extension trait rather than inherent methods, because the type belongs to
/// `nazm-syntax` now. The split is the point: **the front end owns what a built-in *is* —
/// its name, its identity, its signature in language types — and the backend owns what it
/// *becomes*.**
///
/// The names differ from [`Intrinsic`](nazm_syntax::Intrinsic)'s deliberately.
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
        use nazm_syntax::Type;
        // Derived from the front end's own signature rather than listed again: a sequence
        // operation is one whose first parameter is a sequence, and for `ints_new` and
        // `strs_new` — which take nothing — the return type is the only evidence.
        match nazm_syntax::Intrinsic::params(self).first() {
            Some(Type::Ints) => Some(Ty::Ints),
            Some(Type::Strs) => Some(Ty::Strs),
            _ => match nazm_syntax::Intrinsic::ret(self) {
                Type::Ints => Some(Ty::Ints),
                Type::Strs => Some(Ty::Strs),
                _ => None,
            },
        }
    }

    fn lowered_params(self) -> Vec<Ty> {
        nazm_syntax::Intrinsic::params(self)
            .iter()
            .map(|t| Ty::of(*t))
            .collect()
    }

    fn lowered_ret(self) -> Ty {
        Ty::of(nazm_syntax::Intrinsic::ret(self))
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
