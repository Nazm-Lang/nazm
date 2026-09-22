//! The v0.1 syntax tree.
//!
//! Every node carries a span. A node without one produces a diagnostic that cannot point
//! anywhere, and "somewhere in this file" is the class of error this project exists to
//! stop producing.

use nazm_span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
    /// `struct` declarations, in source order.
    ///
    /// A separate list from the functions, because they are a separate namespace: a
    /// module may declare a record `Point` and a function `Point` and they do not
    /// collide. `docs/spec.md`, *Types and functions are separate namespaces*.
    pub records: Vec<Record>,
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

/// `struct Name { field: Type, … }`
///
/// The keyword is `struct` and the category is *record* — one concept with one spelling
/// in source and one in the semantics, chosen so that neither word has to do both jobs.
/// `docs/spec.md` states the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub name_span: Span,
    /// Where `pub` was written, if it was. Syntax, not meaning — exactly as
    /// [`Function::pub_span`].
    pub pub_span: Option<Span>,
    /// The fields, in the order written.
    ///
    /// Order is recorded because a reader wrote it and a formatter must preserve it. It
    /// is **not** part of what the type means: construction is by name, and
    /// `docs/spec.md` makes reordering two fields a no-op for both the interface
    /// fingerprint and the native layout.
    pub fields: Vec<Field>,
    pub span: Span,
}

/// One declared field: `name: Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub name_span: Span,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    /// Where `pub` was written, if it was.
    ///
    /// Syntax, not meaning: this records that the word appeared and where, so a
    /// diagnostic about visibility can point at it. What it *does* — export the
    /// definition to modules that directly import this one — is `nazm_sema`'s to decide,
    /// because deciding it here would put visibility in two places.
    pub pub_span: Option<Span>,
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
    /// `scope { … }` — a block that does not finish until every task spawned inside it
    /// has. A statement, not an expression: it produces no value, and there is nothing a
    /// join could sensibly return.
    Scope(Scope),
    /// `spawn f(args);` — start a task.
    ///
    /// The callee is a **name**, not an expression: the language has no function values,
    /// so there is nothing else it could be, and writing it as a call expression would
    /// suggest otherwise.
    Spawn(Spawn),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawn {
    pub callee: String,
    pub callee_span: Span,
    pub args: Vec<Expr>,
    pub span: Span,
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
            Self::Scope(s) => s.span,
            Self::Spawn(s) => s.span,
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
    /// What is being written to.
    pub place: Place,
    pub value: Expr,
    pub span: Span,
}

/// Where an assignment writes: a binding, then a path of fields through it.
///
/// A **projection path**, not a variable and an optional field. `docs/architecture.md`
/// asks for this shape before it is strictly needed, because the ownership and mutability
/// rules are about a *place*: `a.inner.count = 3` checks mutability at `a` and replaces a
/// field two levels down, and a representation that held one optional field name could not
/// say that. An empty [`Self::path`] is the ordinary `a = …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The root binding's name. Mutability is checked here and nowhere else — see
    /// `docs/spec.md`, *Binding mutability reaches every field*.
    pub name: String,
    pub name_span: Span,
    /// The fields, outermost first: `a.b.c` is `["b", "c"]`.
    pub path: Vec<PlaceField>,
    pub span: Span,
}

/// One step of a projection path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceField {
    pub name: String,
    /// Where the field name is written — the span resolution keys this access by.
    pub name_span: Span,
}

/// One field of a construction: `name: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInit {
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
    /// `Point(x: 1, y: 2)` — construct a record, naming every field.
    ///
    /// Named rather than positional, so that field order is never an implicit API. The
    /// parenthesised form rather than `Point { … }` avoids the ambiguity between a record
    /// literal and the block of an `if` whose condition is a bare name; `docs/spec.md`
    /// records both decisions.
    ///
    /// Distinguished from [`Self::Call`] by two tokens of lookahead — `IDENT (` followed
    /// by `IDENT :` — which is a decision, not a guess: no argument expression can begin
    /// with a name followed by a colon.
    Construct {
        ty: String,
        ty_span: Span,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// `expr.name` — project a field out of a record.
    ///
    /// The base is an expression, not a name, so `make().value` and `a.b.c` are the same
    /// construct nested. What the projection *means* depends on the base's resolved type
    /// and is decided when checking; there is no global field namespace.
    Field {
        base: Box<Expr>,
        name: String,
        /// Where the field name is written — the span resolution keys this access by.
        name_span: Span,
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
            | Self::Construct { span, .. }
            | Self::Field { span, .. }
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
            | Self::Construct { span, .. }
            | Self::Field { span, .. }
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
