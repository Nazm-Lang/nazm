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

/// A sequence of `let` bindings followed by the expression the block evaluates to.
///
/// The tail is required: a block is an expression, and one with no value would have to
/// have a unit type, which v0.1 does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub lets: Vec<Let>,
    pub tail: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Let {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
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
