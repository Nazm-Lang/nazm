//! The v0.1 syntax tree.
//!
//! Every node carries a span. A node without one produces a diagnostic that cannot point
//! anywhere, and "somewhere in this file" is the class of error this project exists to
//! stop producing.

use nazm_span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
    /// `use "path";` items, in source order.
    ///
    /// The parser records them and does nothing else with them: resolving a path means
    /// reading files, which is the driver's job, not the grammar's. By the time a
    /// `Program` reaches the checker every import has already been loaded and merged,
    /// so these are here for tooling and for the diagnostic when one cannot be found.
    pub imports: Vec<Import>,
}

/// `use "path";`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The text between the quotes, exactly as written.
    pub path: String,
    /// The string literal's span, which is where a "cannot find" diagnostic points.
    pub path_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    /// Always written. Inference across a function boundary would make a signature
    /// change invisible at the definition, which is the opposite of what a reader needs.
    pub return_type: TypeName,
    /// `None` when the body failed to parse.
    ///
    /// The signature survives anyway, so calls to this function still check against real
    /// parameter and return types instead of reporting that it does not exist. One
    /// mistake should produce one error.
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName {
    pub name: String,
    pub span: Span,
}

/// Statements, then an optional tail expression.
///
/// One `Block`, not two. Whether it produces a value is decided when checking, from
/// whether there is a tail and whether the statements diverge — not from which syntactic
/// form was parsed. The earlier design had an expression block and a statement block and a
/// forward scanner to guess which one an `if` was, and that guess had a case it got wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub statements: Vec<Stmt>,
    /// Present when the block ends in an expression with no `;` after it.
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let(Let),
    /// `name = value;`
    ///
    /// A statement, not an expression: `if (x = 1) { … }` does not parse, which is one of
    /// the oldest C bugs and nothing is gained by reproducing it.
    Assign(Assign),
    While(While),
    /// Leave the enclosing function with this value.
    Return(Return),
    /// Leave the innermost enclosing loop.
    Break {
        span: Span,
    },
    /// Start the innermost enclosing loop's next iteration, re-evaluating its condition.
    Continue {
        span: Span,
    },
    /// An expression evaluated and discarded.
    ///
    /// This is how an `if` reaches statement position without needing a second `if`.
    Expr(Expr),
}

impl Stmt {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Let(l) => l.span,
            Self::Assign(a) => a.span,
            Self::While(w) => w.span,
            Self::Return(r) => r.span,
            Self::Break { span } | Self::Continue { span } => *span,
            Self::Expr(e) => e.span(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Let {
    pub name: String,
    pub name_span: Span,
    /// Whether the binding was written `let mut`.
    ///
    /// Immutable by default: mutable-by-default with an opt-out would put the annotation
    /// on the rarer case and make the common one silent.
    pub mutable: bool,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct While {
    pub cond: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int {
        value: i64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Str {
        value: String,
        span: Span,
    },
    Var {
        name: String,
        span: Span,
    },
    /// Negation. The only prefix operator in v0.1, and it exists because writing `0 - 1`
    /// for `-1` is the kind of friction that makes a language feel unfinished — a gap the
    /// test suite found by trying to write `i64::MIN`.
    Neg {
        operand: Box<Expr>,
        op_span: Span,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        op_span: Span,
        span: Span,
    },
    Call {
        callee: String,
        callee_span: Span,
        args: Vec<Expr>,
        span: Span,
    },
    If {
        cond: Box<Expr>,
        then_block: Block,
        /// Optional. Whether it is *required* depends on whether a value is wanted from
        /// the `if`, which is a checking question, not a parsing one.
        else_block: Option<Block>,
        span: Span,
    },
    Block(Box<Block>),
}

impl Expr {
    /// The span, by reference, for callers inside a `match` on `self`.
    #[must_use]
    pub fn span_ref(&self) -> &Span {
        match self {
            Self::Int { span, .. }
            | Self::Bool { span, .. }
            | Self::Str { span, .. }
            | Self::Var { span, .. }
            | Self::Neg { span, .. }
            | Self::Binary { span, .. }
            | Self::Call { span, .. }
            | Self::If { span, .. } => span,
            Self::Block(b) => &b.span,
        }
    }

    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Int { span, .. }
            | Self::Bool { span, .. }
            | Self::Str { span, .. }
            | Self::Var { span, .. }
            | Self::Neg { span, .. }
            | Self::Binary { span, .. }
            | Self::Call { span, .. }
            | Self::If { span, .. } => *span,
            Self::Block(b) => b.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl BinOp {
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
        }
    }

    /// Whether the operator produces a `Bool` regardless of its operand type.
    #[must_use]
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::NotEq | Self::Lt | Self::LtEq | Self::Gt | Self::GtEq
        )
    }
}
