//! The v0.1 syntax tree.
//!
//! Every node carries a span. A node without one produces a diagnostic that cannot point
//! anywhere, and "somewhere in this file" is the class of error this project exists to
//! stop producing.

use nazm_span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    /// Always written. Inference across a function boundary would make a signature
    /// change invisible at the definition, which is the opposite of what a reader needs.
    pub return_type: TypeName,
    pub body: Block,
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

/// Statements, then the expression the block evaluates to.
///
/// The tail is required. A block is an expression, and one with no value would need a
/// unit type — a decision with consequences for function returns and every other block,
/// too large to make in passing so that `while` can be an expression. So `while` and
/// assignment are statements, which have no value, and the tail carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub tail: Box<Expr>,
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
}

impl Stmt {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Let(l) => l.span,
            Self::Assign(a) => a.span,
            Self::While(w) => w.span,
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

/// Statements with no tail expression.
///
/// A loop body is this rather than a `Block`, and the distinction is not cosmetic: a
/// `Block` must end in an expression because it *is* one, and a loop body has no value to
/// give. Requiring a tail there would force every loop to end with a pointless expression.
///
/// The spec said `while` is a statement and stopped there; it took writing a loop to
/// notice the body needed saying too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StmtBlock {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct While {
    pub cond: Expr,
    pub body: StmtBlock,
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
        else_block: Block,
        span: Span,
    },
    Block(Box<Block>),
}

impl Expr {
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
