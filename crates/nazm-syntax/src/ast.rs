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
    /// `enum` declarations, in source order.
    ///
    /// A third list rather than one list of items, because the three are three different
    /// shapes and a consumer that wants functions should not have to filter. Records and
    /// enums share the **type namespace** — a module may not declare both under one name
    /// — and the checker, which can see both lists, is what says so.
    pub enums: Vec<Enum>,
    /// `use "path";` items, in source order.
    ///
    /// The parser records them and does nothing else with them: resolving a path means
    /// reading files, which is the driver's job, not the grammar's. By the time a
    /// `Program` reaches the checker every import has already been loaded and merged,
    /// so these are here for tooling and for the diagnostic when one cannot be found.
    pub imports: Vec<Import>,
    /// `trait` declarations, in source order (N78). Traits share the type namespace with
    /// records and enums, and the checker is what says so.
    pub traits: Vec<Trait>,
    /// `impl Trait for Type` headers, in source order (N78). An impl's methods are in
    /// [`Program::functions`], each with [`Function::method_of`] naming its impl.
    pub impls: Vec<ImplDecl>,
    /// `const NAME: Type = value;` declarations, in source order (Gate 2).
    pub consts: Vec<Const>,
    /// The completion hole a probe parse was asked about and found (N26); `None` for every
    /// ordinary parse. See [`HoleSite`].
    pub hole: Option<HoleSite>,
}

/// `@name` or `@name(arguments)` written before a declaration (Gate 2, `docs/spec.md`,
/// *Attributes*). The vocabulary is the compiler's and the checker holds it; here an attribute is
/// its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub name_span: Span,
    /// `None` for `@name`; the arguments, in order, for `@name(…)`.
    pub args: Option<Vec<AttrArg>>,
    pub span: Span,
}

/// One argument of an attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrArg {
    /// `"text"`.
    Str { value: String, span: Span },
    /// `word`.
    Word { name: String, span: Span },
    /// `key = "value"`.
    Pair {
        key: String,
        key_span: Span,
        value: String,
        span: Span,
    },
    /// `name(arguments)`: `all(…)`, `any(…)`, `not(…)`.
    Call {
        name: String,
        name_span: Span,
        args: Vec<AttrArg>,
        span: Span,
    },
}

impl AttrArg {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Str { span, .. }
            | Self::Word { span, .. }
            | Self::Pair { span, .. }
            | Self::Call { span, .. } => *span,
        }
    }
}

/// `[pub] trait Name { fn m(self: Self, …) -> R; … }` (N78, `architecture.md` §7.79).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trait {
    /// The attributes written before it (Gate 2).
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub name_span: Span,
    pub pub_span: Option<Span>,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

/// One method a trait requires: a signature, no body. Its first parameter is `self: Self`,
/// which the checker holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethod {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: TypeName,
    pub effects: Option<EffectDecl>,
    pub span: Span,
}

/// The header of `impl Trait for Type { … }` (N78). The methods are functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDecl {
    /// The attributes written before it (Gate 2).
    pub attrs: Vec<Attribute>,
    pub trait_name: TypeName,
    pub self_ty: TypeName,
    /// The `impl` keyword: what the impl is identified by in diagnostics, and what each of
    /// its methods' [`MethodOf::impl_span`] names.
    pub impl_span: Span,
    pub span: Span,
}

/// Which impl a function is a method of (N78).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodOf {
    pub trait_name: TypeName,
    pub self_ty: TypeName,
    pub impl_span: Span,
}

/// Where a completion probe's hole is, as the parser read it (N26).
///
/// Only a parse told the byte offset of an editor's cursor makes one, and only at exactly
/// that offset, right after a `.` or a `(` with nothing written after it. The hole is a
/// zero-width, empty name, which no source can spell; the tree around it is what the
/// grammar reads, and the checker decides what it means. Never produced by `nazm check`,
/// `nazm build` or the language service's ordinary analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoleSite {
    /// After `.`: the empty member name of a projection — or, after `Name[T, …].`, the
    /// empty variant name of a variant construction.
    Member { name: Span },
    /// After `Name(`, with no label written: a call of `Name`, which is how the grammar
    /// reads a `(` without labels.
    Call { callee: Span },
    /// After `E.V(`, in a variant construction or (`pattern`) a match pattern.
    Payload { variant: Span, pattern: bool },
}

/// `const NAME: Type = value;`, optionally `pub` (Gate 2, `docs/spec.md`, *Constants*): a
/// value the checker computes, usable wherever an expression is and as an array's length.
/// `const` is a word only here, at the start of an item, so no name a program already uses
/// stops being one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Const {
    /// The attributes written before it (Gate 2).
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub name_span: Span,
    pub pub_span: Option<Span>,
    pub ty: TypeName,
    pub value: Expr,
    pub span: Span,
    /// Written `static` rather than `const` (Gate 3, `docs/spec.md`, *Statics*): storage with
    /// one address, read-only unless an `Atomic`, whose value the checker computes as a
    /// constant's — or `atomic_new` of one. Everything a constant's name does, a static's does,
    /// except stand for an array's length.
    pub is_static: bool,
}

/// `use "path";` or `use "path" as name;`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The text between the quotes, exactly as written.
    pub path: String,
    /// The string literal's span, which is where a "cannot find" diagnostic points.
    pub path_span: Span,
    /// `as name` (N49): the module is imported under this name, and its exports are written
    /// `name::item` rather than brought into scope.
    pub alias: Option<Qualifier>,
    /// `pub use "path";` or `pub use "path" { a, b as c };` (N103): what this module offers
    /// of the imported module's exports as its own. `None` for an ordinary import.
    pub reexport: Option<Reexport>,
    pub span: Span,
}

/// What a `pub use` re-exports (N103). A re-export offers the **same definition** under this
/// module's name: no second definition is made, and every identity — durable or session —
/// is the one the defining module gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reexport {
    /// The `pub` keyword's span.
    pub pub_span: Span,
    /// The names chosen, or `None` for every export of the module.
    pub names: Option<Vec<ReexportName>>,
}

/// One name a `pub use` re-exports: as it is exported there, and — with `as` — the name this
/// module offers it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReexportName {
    pub name: String,
    pub span: Span,
    pub alias: Option<(String, Span)>,
}

/// A function type as written (N50): `fn(Int, Str) -> Bool ! {io}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnTypeName {
    pub params: Vec<TypeName>,
    pub ret: TypeName,
    /// `! { … }`, or `None`: none written means no effects.
    pub effects: Option<EffectDecl>,
}

/// A module alias written before `::` (N49): `t` in `t::text_split`. Its span is the alias's
/// alone; the item's name keeps its own span, which is what resolution is keyed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qualifier {
    pub name: String,
    pub span: Span,
}

/// `struct Name { field: Type, … }`
///
/// The keyword is `struct` and the category is *record* — one concept with one spelling
/// in source and one in the semantics, chosen so that neither word has to do both jobs.
/// `docs/spec.md` states the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The attributes written before it (Gate 2).
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub name_span: Span,
    /// Where `pub` was written, if it was. Syntax, not meaning — exactly as
    /// [`Function::pub_span`].
    pub pub_span: Option<Span>,
    /// `[A, B]` after the name, in the order written. Empty for a record that is not
    /// generic. Order is semantic and spelling is not: `docs/spec.md`, *Order is semantic;
    /// names are not*.
    pub type_params: Vec<TypeParam>,
    /// The fields, in the order written.
    ///
    /// Order is recorded because a reader wrote it and a formatter must preserve it. It
    /// is **not** part of what the type means: construction is by name, and
    /// `docs/spec.md` makes reordering two fields a no-op for both the interface
    /// fingerprint and the native layout.
    pub fields: Vec<Field>,
    /// `extern "C" struct …` (N85): a record C sees. `None` for every ordinary record.
    pub foreign: Option<ForeignStruct>,
    pub span: Span,
}

/// The foreign half of an `extern "C" struct` declaration, as written (N85).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignStruct {
    pub abi: String,
    pub abi_span: Span,
    /// `extern "C" struct Name;` — no field list: an opaque handle, a C pointer Nazm never
    /// reads. With a field list it is a C-layout record.
    pub opaque: bool,
}

/// One declared field: `name: Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub name_span: Span,
    pub ty: TypeName,
    pub span: Span,
}

/// `enum Name { Variant, Other(field: Type), … }`
///
/// One keyword and one category, unlike `struct`/record: `docs/spec.md` says why there is
/// no second spelling for either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    /// The attributes written before it (Gate 2).
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub name_span: Span,
    /// Where `pub` was written, if it was. Syntax, not meaning.
    pub pub_span: Option<Span>,
    /// `[T]` after the name. Empty for an enum that is not generic.
    pub type_params: Vec<TypeParam>,
    /// The variants, in the order written.
    ///
    /// Order is recorded because a reader wrote it and a formatter must preserve it. It is
    /// **not** part of what the type means: `docs/spec.md` makes reordering two variants a
    /// no-op for the language, for the interface fingerprint and for the discriminant the
    /// backend assigns.
    pub variants: Vec<Variant>,
    pub span: Span,
}

/// One declared variant: `Name` or `Name(field: Type, …)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    pub name_span: Span,
    /// The payload fields, in the order written. Empty for a zero-payload variant, which
    /// is written with **no parentheses** — there is no field list, exactly as a record
    /// with no fields has none. Construction and patterns always write them, because
    /// there the parentheses are what separates a variant from a field projection.
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The attributes written before it (Gate 2); empty for an impl's method.
    pub attrs: Vec<Attribute>,
    pub name: String,
    pub name_span: Span,
    /// Where `pub` was written, if it was.
    ///
    /// Syntax, not meaning: this records that the word appeared and where, so a
    /// diagnostic about visibility can point at it. What it *does* — export the
    /// definition to modules that directly import this one — is `nazm_sema`'s to decide,
    /// because deciding it here would put visibility in two places.
    pub pub_span: Option<Span>,
    /// `[T]` after the name. Empty for a function that is not generic.
    pub type_params: Vec<TypeParam>,
    /// `effects E` in the same brackets (N51), after the type parameters: the function's
    /// effect parameters, as written. The language allows one; how many may be written is
    /// the checker's question, so a second parses and is refused there.
    pub effect_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    /// Always written. Inference across a function boundary would make a signature
    /// change invisible at the definition, which is the opposite of what a reader needs.
    pub return_type: TypeName,
    /// `! { … }` after the return type, N36: the effects the function declares it may
    /// exercise. `None` where nothing was written — which is not "no effects": the
    /// checker infers an undeclared function's effects from its body. What the names
    /// mean, and whether they exist, is `nazm_sema`'s to decide; here they are words.
    pub effects: Option<EffectDecl>,
    /// `requires E` and `ensures E` clauses (N87, `docs/architecture.md` §7.88), in the order
    /// written, every `requires` before every `ensures`. Empty for most functions.
    pub contracts: Vec<Contract>,
    /// `None` when the body failed to parse.
    ///
    /// The signature survives anyway, so calls to this function still check against real
    /// parameter and return types instead of reporting that it does not exist. One
    /// mistake should produce one error.
    pub body: Option<Block>,
    /// `extern "C" … = "symbol";` (N42): a foreign function, declared and not defined here,
    /// whose body is the C function the symbol names. `None` for every Nazm function. A
    /// foreign declaration has no `body`, and that absence is not a parse failure.
    pub foreign: Option<ForeignDecl>,
    /// `extern "C" … = "symbol" { … }` (N55): a Nazm function with a body that C may also call,
    /// under `symbol`. `None` for every other function; never set together with `foreign`.
    pub export: Option<ForeignDecl>,
    /// The impl this function is a method of, for one declared inside `impl … { }` (N78).
    pub method_of: Option<Box<MethodOf>>,
    pub span: Span,
}

/// One contract clause (N87): what a function promises its callers, or asks of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub kind: ContractKind,
    /// The `requires` or `ensures` keyword.
    pub keyword_span: Span,
    pub expr: Expr,
    pub span: Span,
}

/// Which side of a call a contract clause is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContractKind {
    /// Checked when the function is entered.
    Requires,
    /// Checked when it returns, with `result` the value returned.
    Ensures,
}

/// The foreign half of an `extern "C" fn … = "symbol";` declaration, as written (N42).
///
/// Words and their spans: whether the ABI is one this compiler calls and whether the symbol
/// is one a linker can be handed are `nazm_sema`'s and the checker's questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignDecl {
    pub abi: String,
    pub abi_span: Span,
    pub symbol: String,
    pub symbol_span: Span,
}

/// A declared effect set as written: `! {}`, `! { io }`, `! { io, spawn }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDecl {
    /// From the `!` to the closing `}`.
    pub span: Span,
    /// Each name as written, in source order, duplicates and all: a duplicate is the
    /// checker's to report, and it can only if the parser keeps it.
    pub names: Vec<(String, Span)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// The name as written. N16: where a use of the parameter resolves to.
    pub name_span: Span,
    pub ty: TypeName,
    pub span: Span,
}

/// A type as written: a name, and — for an application — its arguments.
///
/// Recursive since N11: `Vec[Pair[Int, Str]]` is a name with one argument that is itself a
/// name with two. `args` is `None` where no brackets were written and `Some` where they
/// were, **including empty ones**: `Vec[]` is a type written with the wrong number of
/// arguments, which is a question for the checker about a type rather than one for the
/// parser about punctuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName {
    /// `m::` before the name (N49).
    pub qualifier: Option<Qualifier>,
    /// `fn(T…) -> R ! {e}` (N50): a function type, whose `name` is `fn`.
    pub function: Option<Box<FnTypeName>>,
    pub name: String,
    /// Where the name alone is written, for a diagnostic about the name.
    pub name_span: Span,
    /// The whole written type, arguments included.
    pub span: Span,
    pub args: Option<Vec<TypeName>>,
    /// `[T; N]` (Gate 2): a fixed-size array, whose `name` is `[]`, whose one argument is the
    /// element type, and whose length is this — an integer literal or a constant's name.
    pub length: Option<Box<Expr>>,
}

impl TypeName {
    /// A plain name with no arguments, for code that builds one.
    #[must_use]
    pub fn plain(name: impl Into<String>, span: Span) -> Self {
        Self {
            qualifier: None,
            function: None,
            name: name.into(),
            name_span: span,
            span,
            args: None,
            length: None,
        }
    }

    /// The name an array type is written under in a [`TypeName`]: `[T; N]`.
    pub const ARRAY: &'static str = "[]";
    /// The name of a tuple type, `(A, B, …)` (R1a), whose arguments are its elements.
    pub const TUPLE: &'static str = "()";
}

/// One declared type parameter: `T` in `fn identity[T](…)`, or `T: Equality` in
/// `fn same[T: Equality](…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParam {
    pub name: String,
    pub span: Span,
    /// Written `effects NAME` (N51): an effect parameter, not a type parameter. Only a
    /// function's list may hold one.
    pub effect: bool,
    /// The capability the parameter is required to have, as written — `Equality` — or
    /// `None`. N14. Which names mean a capability, and where a requirement may be written,
    /// is the checker's question.
    pub requires: Option<Requirement>,
}

/// A written capability requirement on a type parameter: the name after the `:`. N14.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
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

impl Block {
    /// The closure expression written at `at` in this block, at any depth (N50): how a layer
    /// that holds a closure's span finds its syntax again. Iterative, so a deeply nested body
    /// costs no stack.
    #[must_use]
    pub fn closure_at(&self, at: Span) -> Option<&Expr> {
        let mut exprs: Vec<&Expr> = Vec::new();
        let mut blocks: Vec<&Self> = vec![self];
        loop {
            if let Some(b) = blocks.pop() {
                for st in &b.statements {
                    match st {
                        Stmt::Let(l) => exprs.push(&l.value),
                        Stmt::Assign(a) => exprs.push(&a.value),
                        Stmt::While(w) => {
                            exprs.push(&w.cond);
                            blocks.push(&w.body);
                        }
                        Stmt::For(f) => {
                            exprs.extend(f.iter.exprs());
                            blocks.push(&f.body);
                        }
                        Stmt::Return(r) => exprs.push(&r.value),
                        Stmt::Expr(e) => exprs.push(e),
                        Stmt::Scope(sc) => blocks.push(&sc.body),
                        Stmt::Spawn(sp) => exprs.extend(sp.args.iter()),
                        Stmt::Break { .. } | Stmt::Continue { .. } => {}
                    }
                }
                if let Some(t) = &b.tail {
                    exprs.push(t);
                }
                continue;
            }
            let e = exprs.pop()?;
            let span = e.span();
            if span.file != at.file || span.start > at.start || span.end < at.end {
                continue;
            }
            match e {
                Expr::Closure { span, body, .. } => {
                    if *span == at {
                        return Some(e);
                    }
                    blocks.push(body);
                }
                Expr::Int { .. }
                | Expr::WideInt { .. }
                | Expr::Float { .. }
                | Expr::Bool { .. }
                | Expr::Str { .. }
                | Expr::Var { .. } => {}
                Expr::Neg { operand, .. }
                | Expr::Not { operand, .. }
                | Expr::BitNot { operand, .. }
                | Expr::Propagate { operand, .. } => exprs.push(operand),
                Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => exprs.extend(elems.iter()),
                Expr::ArrayRepeat { value, count, .. } => {
                    exprs.push(value);
                    exprs.push(count);
                }
                Expr::Index { base, index, .. } => {
                    exprs.push(base);
                    exprs.push(index);
                }
                Expr::Field { base, .. } => exprs.push(base),
                // An interpolation's holes (L4).
                Expr::Interp { parts, .. } => {
                    for p in parts {
                        if let InterpPart::Hole { expr, .. } = p {
                            exprs.push(expr);
                        }
                    }
                }
                Expr::Binary { lhs, rhs, .. } => {
                    exprs.push(lhs);
                    exprs.push(rhs);
                }
                Expr::Call { args, .. } => exprs.extend(args.iter()),
                Expr::MethodCall { receiver, args, .. } => {
                    exprs.push(receiver);
                    exprs.extend(args.iter());
                }
                Expr::Construct { fields, .. } | Expr::Variant { fields, .. } => {
                    exprs.extend(fields.iter().map(|f| &f.value));
                }
                Expr::Match {
                    scrutinee, arms, ..
                } => {
                    exprs.push(scrutinee);
                    exprs.extend(arms.iter().map(|a| &a.body));
                }
                Expr::If {
                    cond,
                    then_block,
                    else_block,
                    ..
                } => {
                    exprs.push(cond);
                    blocks.push(then_block);
                    if let Some(b) = else_block {
                        blocks.push(b);
                    }
                }
                Expr::Block(b) => blocks.push(b),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
// Sized by its largest variant on purpose, like the parser's `Item`: a statement is built once
// and moved into its block, so boxing a variant would be one allocation more and nothing saved.
#[allow(clippy::large_enum_variant)]
pub enum Stmt {
    Let(Let),
    /// `name = value;`
    ///
    /// A statement, not an expression: `if (x = 1) { … }` does not parse, which is one of
    /// the oldest C bugs and nothing is gained by reproducing it.
    Assign(Assign),
    While(While),
    /// `for NAME in e { … }` or `for NAME in a..b { … }` (R1a): each the `while` loop
    /// `docs/spec.md` (*`for` loops*) gives for it.
    For(For),
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
            Self::For(f) => f.span,
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
    /// The fields and indexes, outermost first: `a.b[i].c` is `[b, [i], c]`.
    pub path: Vec<PlaceStep>,
    pub span: Span,
}

/// One step of an assignment's place: a field, or an array element (Gate 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceStep {
    Field(PlaceField),
    /// `[index]`; `span` covers the brackets.
    Index {
        index: Box<Expr>,
        span: Span,
    },
}

/// One field step of a projection path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceField {
    pub name: String,
    /// Where the field name is written — the span resolution keys this access by.
    pub name_span: Span,
}

/// One arm of a `match`: a pattern and the expression it selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// A pattern (`docs/spec.md`, *Enums* and, since R1a, *Patterns*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `_`: matches anything and binds nothing. It declines to *name* a value; the value is
    /// still there and still released with what holds it.
    Wildcard { span: Span },
    /// `name`: matches anything and binds it, immutably (R1a at the top level and nested; a
    /// payload field's binding before R1a).
    Bind { name: String, span: Span },
    /// An integer (perhaps negated), a string, `true` or `false` (R1a): matches a value equal
    /// to it by its type's equality.
    Literal(Expr),
    /// `Enum.Variant(field: pattern, …)`.
    Variant(VariantPattern),
    /// `p | q | …` (R1a): matches what any alternative matches; each binds the same names.
    Or { alts: Vec<Pattern>, span: Span },
    /// `(p, q, …)` (R1a, L5): a tuple whose elements match.
    Tuple { elems: Vec<Pattern>, span: Span },
}

impl Pattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Wildcard { span }
            | Self::Bind { span, .. }
            | Self::Or { span, .. }
            | Self::Tuple { span, .. } => *span,
            Self::Literal(e) => e.span(),
            Self::Variant(v) => v.span,
        }
    }

    /// Every name this pattern binds, with where, in the order written — the first
    /// alternative's, for an or-pattern, whose alternatives bind the same names.
    #[must_use]
    pub fn bindings(&self) -> Vec<(&str, Span)> {
        let mut out = Vec::new();
        let mut work = vec![self];
        while let Some(p) = work.pop() {
            match p {
                Self::Bind { name, span } => out.push((name.as_str(), *span)),
                Self::Variant(v) => work.extend(v.fields.iter().rev().map(|f| &f.pattern)),
                Self::Tuple { elems, .. } => work.extend(elems.iter().rev()),
                Self::Or { alts, .. } => work.extend(alts.first()),
                Self::Wildcard { .. } | Self::Literal(_) => {}
            }
        }
        out
    }
}

/// `Enum.Variant(field: pattern, …)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantPattern {
    /// The enum named before the `.`. Resolved against the type namespace, and required to
    /// be the scrutinee's own enum — there is no structural variant matching.
    /// `m::` before the enum's name (N49).
    pub qualifier: Option<Qualifier>,
    pub ty: String,
    pub ty_span: Span,
    pub variant: String,
    /// Where the variant name is written — the span resolution keys this pattern by.
    pub variant_span: Span,
    /// Every payload field of the variant, exactly once. `docs/spec.md` requires the
    /// pattern to account for all of them, so that adding a payload field breaks old
    /// patterns instead of letting them ignore new state.
    pub fields: Vec<PatternField>,
    pub span: Span,
}

/// One field of a variant pattern: `name: pattern` — before R1a, `name: binding` or `name: _`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternField {
    /// The payload field's name, which is how it is identified.
    pub name: String,
    /// Where the field name is written — the span resolution keys this access by.
    pub name_span: Span,
    /// What the payload must match.
    ///
    /// `_` means *introduce no binding*; it does not mean the field is not there and does
    /// not mean it is not released. `docs/spec.md` says so, because the opposite reading
    /// is the one that leaks.
    pub pattern: Pattern,
}

/// Where a `match` came from (R1a): written, or what `if let` or `let … else` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    Written,
    /// `if let P = e { A } else { B }`: arms `P => A` and `_ => B`.
    IfLet,
    /// `let P = e else { D }; rest`: arms `P => { rest }` and `_ => D`, where `D` must leave.
    LetElse,
    /// `let P = e; rest` (L5): one arm `P => { rest }`, so `P` must match every value.
    Let,
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

/// A `for` loop (R1a): its name, what it iterates, and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct For {
    /// The name each step binds, immutable and new each step.
    pub name: String,
    pub name_span: Span,
    pub iter: ForIter,
    pub body: Block,
    pub span: Span,
}

/// What a `for` loop iterates.
#[derive(Debug, Clone, PartialEq, Eq)]
// Sized by its largest variant on purpose: a loop has exactly one, built once.
#[allow(clippy::large_enum_variant)]
pub enum ForIter {
    /// `in e`: an `Ints`, `Strs`, `Vec[T]` or `Bytes`, evaluated once.
    Each(Expr),
    /// `in a..b`: the `Int`s from `a` up to, not including, `b`; each evaluated once.
    Range { start: Expr, end: Expr },
}

impl ForIter {
    /// The expressions written, in evaluation order.
    #[must_use]
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            Self::Each(e) => vec![e],
            Self::Range { start, end } => vec![start, end],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int {
        value: i64,
        span: Span,
    },
    /// An integer literal above `Int`'s range and within `UInt64`'s (Gate 2): valid only where a
    /// `UInt64` is expected, and refused as too large for `Int` everywhere else.
    WideInt {
        value: u64,
        span: Span,
    },
    /// A floating-point literal (Gate 2), as written with any `_` removed. Its type — `Float64`,
    /// or `Float32` where one is expected — decides how it rounds.
    Float {
        text: String,
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
    /// `fn(x: Int) -> Int ! {e} { body }` — a closure (N50). Its parameters and result are
    /// written in full, its effects declared or none; the bindings its body names from the
    /// functions around it are captured by copy. `fn_span` is the `fn` keyword's.
    Closure {
        params: Vec<Param>,
        return_type: TypeName,
        effects: Option<EffectDecl>,
        body: Box<Block>,
        fn_span: Span,
        span: Span,
    },
    /// `!operand` — logical negation of a `Bool` (N49). Core IR lowers it to
    /// `if operand { false } else { true }`; nothing below the checker sees it.
    Not {
        operand: Box<Expr>,
        op_span: Span,
        span: Span,
    },
    /// `[a, b, c]` — a fixed-size array of the elements written, in order (Gate 2).
    Array {
        elems: Vec<Expr>,
        span: Span,
    },
    /// `[value; count]` — `count` copies of `value`; `count` is a literal or a constant.
    ArrayRepeat {
        value: Box<Expr>,
        count: Box<Expr>,
        span: Span,
    },
    /// `base[index]` — an array's element, by an `Int` index (Gate 2).
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        /// The brackets, which a bounds failure points at.
        bracket_span: Span,
        span: Span,
    },
    /// `~operand` — the bit complement of an integer (Gate 2).
    BitNot {
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
        /// `m::` before the callee (N49).
        qualifier: Option<Qualifier>,
        callee: String,
        callee_span: Span,
        /// `f[Int](…)` — explicit type arguments, or `None` where none were written.
        type_args: Option<Vec<TypeName>>,
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
        /// `m::` before the record's name (N49).
        qualifier: Option<Qualifier>,
        ty: String,
        ty_span: Span,
        /// `Pair[Int, Str](…)` — the type arguments of a generic record, which are always
        /// written. `None` where none were.
        type_args: Option<Vec<TypeName>>,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// `Token.Number(value: 42)` — build a value of an enum by naming one of its variants.
    ///
    /// Always qualified by the enum, and always parenthesised, including for a variant
    /// with no payload: `docs/spec.md` gives both reasons. `IDENT . IDENT (` is what
    /// distinguishes this from a projection, and it is a decision rather than a guess —
    /// there are no methods and no function values, so nothing else can put a `(` after a
    /// field read.
    Variant {
        /// `m::` before the enum's name (N49).
        qualifier: Option<Qualifier>,
        ty: String,
        ty_span: Span,
        /// `Maybe[Int].Some(…)` — the type arguments of a generic enum, which are always
        /// written. `None` where none were.
        type_args: Option<Vec<TypeName>>,
        variant: String,
        /// Where the variant name is written — the span resolution keys this by.
        variant_span: Span,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// `match scrutinee { Enum.Variant(field: binding) => body, … }`
    ///
    /// An expression, because the language has one notion of a value-producing construct
    /// and a second statement-only form would be a second set of completion rules.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        /// Written, or what `if let` or `let … else` means (R1a).
        source: MatchSource,
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
    /// `operand?` — propagation, N12.
    ///
    /// On the core prelude's `Result.Ok` it is the payload; on `Result.Err` it leaves the
    /// enclosing function, returning that error value in the function's own `Result`.
    /// Ordinary control flow over an ordinary enum: `docs/spec.md`, *Propagation*.
    Propagate {
        operand: Box<Expr>,
        /// Where the `?` is written — the span resolution keys this propagation by, and
        /// what a diagnostic about it points at.
        op_span: Span,
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
    /// `(a, b, …)` (R1a, L5): a tuple of two or more values, evaluated in order.
    Tuple {
        elems: Vec<Expr>,
        span: Span,
    },
    Block(Box<Block>),
    /// `receiver.method(args)`, or `Trait.method(receiver, args)` written with a trait's
    /// name as the receiver (N78). Which it is, and which function it calls, is the
    /// checker's to decide; `E.A()` with no arguments parses as a variant and the checker
    /// reads it as a call when `E` is a value.
    MethodCall {
        qualifier: Option<Qualifier>,
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<Expr>,
        span: Span,
    },
    /// `f"…{x}…"` — an interpolated string (R1a, L4): its text and holes, in order.
    Interp {
        parts: Vec<InterpPart>,
        span: Span,
    },
}

/// A piece of an interpolated string (L4).
#[derive(Debug, Clone, PartialEq, Eq)]
// Sized by its largest variant on purpose: a part is built once, in place, in its string.
#[allow(clippy::large_enum_variant)]
pub enum InterpPart {
    /// Text, as it reads.
    Text(String),
    /// A hole: its expression, its format, and the span of its braces — which the checker keys
    /// what it decides about the hole by.
    Hole {
        expr: Expr,
        spec: crate::lexer::FormatSpec,
        span: Span,
    },
}

impl Expr {
    /// The span, by reference, for callers inside a `match` on `self`.
    #[must_use]
    pub fn span_ref(&self) -> &Span {
        match self {
            Self::Int { span, .. }
            | Self::WideInt { span, .. }
            | Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::Str { span, .. }
            | Self::Var { span, .. }
            | Self::Neg { span, .. }
            | Self::Not { span, .. }
            | Self::BitNot { span, .. }
            | Self::Array { span, .. }
            | Self::ArrayRepeat { span, .. }
            | Self::Index { span, .. }
            | Self::Closure { span, .. }
            | Self::Binary { span, .. }
            | Self::Call { span, .. }
            | Self::Construct { span, .. }
            | Self::Variant { span, .. }
            | Self::Match { span, .. }
            | Self::Tuple { span, .. }
            | Self::Field { span, .. }
            | Self::Propagate { span, .. }
            | Self::If { span, .. }
            | Self::Interp { span, .. }
            | Self::MethodCall { span, .. } => span,
            Self::Block(b) => &b.span,
        }
    }

    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Int { span, .. }
            | Self::WideInt { span, .. }
            | Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::Str { span, .. }
            | Self::Var { span, .. }
            | Self::Neg { span, .. }
            | Self::Not { span, .. }
            | Self::BitNot { span, .. }
            | Self::Array { span, .. }
            | Self::ArrayRepeat { span, .. }
            | Self::Index { span, .. }
            | Self::Closure { span, .. }
            | Self::Binary { span, .. }
            | Self::Call { span, .. }
            | Self::Construct { span, .. }
            | Self::Variant { span, .. }
            | Self::Match { span, .. }
            | Self::Tuple { span, .. }
            | Self::Field { span, .. }
            | Self::Propagate { span, .. }
            | Self::If { span, .. }
            | Self::Interp { span, .. }
            | Self::MethodCall { span, .. } => *span,
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
    /// `&&` and `||` (N49): `Bool` operands, short-circuit, lowered by Core IR to `if`.
    And,
    Or,
    /// `&`, `|`, `^`, `<<`, `>>` (Gate 2): two operands of one integer type; a shift's amount
    /// is an `Int`.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
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
            Self::And => "&&",
            Self::Or => "||",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::Shl => "<<",
            Self::Shr => ">>",
        }
    }

    /// Whether the operator is `&&` or `||`: `Bool` operands, and the right one evaluated
    /// only when the left does not decide the result.
    #[must_use]
    pub fn is_logical(self) -> bool {
        matches!(self, Self::And | Self::Or)
    }

    /// `==` or `!=`.
    #[must_use]
    pub fn is_equality(self) -> bool {
        matches!(self, Self::Eq | Self::NotEq)
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
