//! The lossless concrete syntax tree. N15.
//!
//! # What it is for
//!
//! The abstract tree ([`crate::ast`]) is what the program *means*: it has no whitespace, no
//! comments, no parentheses and no punctuation, and a function whose body did not parse has
//! no body. That is right for a checker and wrong for anything that edits source — a tool
//! that renames a function, moves a statement or reports on a half-written file needs the
//! file as written, including the parts that are wrong. This tree is that file.
//!
//! Its responsibilities are exactly these: the source structure as written, every byte of
//! trivia, the structure of malformed regions, and stable node boundaries a later edit or a
//! language server can address. It is **not** a second semantic tree. Nothing downstream of
//! the parser reads it; names are not resolved in it and types are not in it.
//!
//! # Every byte belongs to exactly one leaf
//!
//! The leaves, in order, are the input: [`Cst::text`] concatenates them and is the input,
//! byte for byte — not normalised, not re-spelled from token kinds. Each leaf's text is the
//! slice of the source it was scanned from (see [`Syntax::static_text`] below for why no
//! kind is allowed a fixed spelling). A malformed region is an [`SyntaxKind::Error`] node
//! over the bytes it covers; an unknown character is an [`SyntaxKind::Invalid`] leaf. There
//! is no synthetic token: a missing `;` is a diagnostic, not a zero-width leaf.
//!
//! # Where it is built
//!
//! By the one parser, from the one lossless scan ([`crate::lexer::lex_lossless_in`]). Every
//! token the parser consumes goes through a single `bump`, which is also what puts its leaf
//! in this tree, and every construct the parser recognises is wrapped as it is recognised.
//! The abstract tree is built by the same decisions in the same functions; see `parser.rs`.
//!
//! # The library
//!
//! `cstree` 0.14 (`architecture.md` §8): a green tree over interned token text, with a red
//! layer on top. Its types stay in this module — the rest of the workspace sees [`Cst`],
//! [`CstNode`], [`Leaf`] and [`SyntaxKind`].

use std::sync::Arc;

use cstree::build::GreenNodeBuilder;
use cstree::green::{GreenNode, GreenToken};
use cstree::interning::{InternKey, Interner, Resolver, TokenKey};
use cstree::syntax::{ResolvedNode, SyntaxNode};
use cstree::text::TextRange;
use cstree::util::NodeOrToken;
use cstree::{RawSyntaxKind, Syntax};
use nazm_span::{FileId, Span};

use crate::lexer::{Lexeme, TokenKind};

/// Every kind a node or a leaf of the concrete tree can have.
///
/// Three groups, kept apart by [`SyntaxKind::is_token`] and [`SyntaxKind::is_trivia`]:
/// the language's tokens, one per [`TokenKind`]; trivia and invalid bytes; and the nodes,
/// one per grammar construct in `docs/grammar.ebnf`, plus [`SyntaxKind::Error`].
///
/// A node kind names a construct of the grammar, not a function of the parser. There is a
/// [`SyntaxKind::ParenExpr`] although the abstract tree has no parenthesis, because the
/// parentheses are in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u32)]
pub enum SyntaxKind {
    // --- tokens: literals and names ---
    Int,
    /// A floating-point literal (Gate 2).
    Float,
    Str,
    Ident,
    // --- tokens: keywords ---
    FnKw,
    LetKw,
    IfKw,
    ElseKw,
    TrueKw,
    FalseKw,
    MutKw,
    PubKw,
    WhileKw,
    BreakKw,
    ContinueKw,
    ReturnKw,
    UseKw,
    ScopeKw,
    SpawnKw,
    StructKw,
    EnumKw,
    MatchKw,
    TraitKw,
    ImplKw,
    // --- tokens: punctuation and operators ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    /// `..`, between a `for` loop's bounds (R1a).
    DotDot,
    Semi,
    Question,
    Bang,
    AmpAmp,
    PipePipe,
    ColonColon,
    Arrow,
    FatArrow,
    Eq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    /// `@`, which begins a declaration's attribute (Gate 2).
    At,
    // --- trivia, and bytes that are not Nazm ---
    Whitespace,
    Comment,
    /// Bytes the lexer reported and could not make a token of.
    Invalid,
    // --- nodes ---
    SourceFile,
    UseItem,
    FnItem,
    RecordItem,
    EnumItem,
    TypeParamList,
    TypeParam,
    ParamList,
    Param,
    RetType,
    /// `! { io, spawn }` after a return type (N36).
    EffectSet,
    /// `requires E` or `ensures E` (N87).
    Contract,
    Type,
    TypeArgList,
    /// `{ name: Type, … }` of a record, or `( name: Type, … )` of a variant's payload.
    FieldList,
    Field,
    VariantList,
    Variant,
    Block,
    LetStmt,
    AssignStmt,
    /// The left side of an assignment: `name` or `name.field.field`.
    Place,
    WhileStmt,
    /// `let P = e else { … };` (R1a).
    LetElseStmt,
    /// `for NAME in e { … }` or `for NAME in a..b { … }` (R1a).
    ForStmt,
    ReturnStmt,
    BreakStmt,
    ContinueStmt,
    ScopeStmt,
    SpawnStmt,
    ExprStmt,
    Literal,
    NameRef,
    NegExpr,
    NotExpr,
    /// `~x` — bit complement (Gate 2).
    BitNotExpr,
    ClosureExpr,
    FnType,
    BinaryExpr,
    ParenExpr,
    CallExpr,
    ArgList,
    ConstructExpr,
    VariantExpr,
    FieldInitList,
    FieldInit,
    FieldExpr,
    PropagateExpr,
    IfExpr,
    /// `if let P = e { … } else { … }` (R1a).
    IfLetExpr,
    MatchExpr,
    MatchArmList,
    MatchArm,
    Pattern,
    /// `p | q | …` (R1a).
    OrPattern,
    PatternFieldList,
    PatternField,
    /// `trait Name { … }` (N78).
    TraitItem,
    /// One method of a trait: a signature ending in `;`.
    TraitMethod,
    /// `impl Trait for Type { … }` (N78).
    ImplItem,
    /// `receiver.method(args)` (N78).
    MethodCallExpr,
    /// `const NAME: Type = value;` (Gate 2).
    ConstItem,
    /// `[T; N]` (Gate 2).
    ArrayType,
    /// `[a, b]` and `[x; N]` (Gate 2).
    ArrayExpr,
    /// `base[index]` (Gate 2).
    IndexExpr,
    /// `@name` or `@name(arguments)` before a declaration (Gate 2).
    Attribute,
    /// A region that did not parse: the partial construct and the tokens skipped to the
    /// next place the grammar can resume. Its bytes are the author's, unchanged.
    Error,
}

impl SyntaxKind {
    /// Every kind, in declaration order, so the raw value of a kind is its index here.
    ///
    /// The only integer conversion the tree needs goes through this table, in both
    /// directions, and a test checks that each entry's raw value is its position.
    pub const ALL: &'static [Self] = &[
        Self::Int,
        Self::Float,
        Self::Str,
        Self::Ident,
        Self::FnKw,
        Self::LetKw,
        Self::IfKw,
        Self::ElseKw,
        Self::TrueKw,
        Self::FalseKw,
        Self::MutKw,
        Self::PubKw,
        Self::WhileKw,
        Self::BreakKw,
        Self::ContinueKw,
        Self::ReturnKw,
        Self::UseKw,
        Self::ScopeKw,
        Self::SpawnKw,
        Self::StructKw,
        Self::EnumKw,
        Self::MatchKw,
        Self::TraitKw,
        Self::ImplKw,
        Self::LParen,
        Self::RParen,
        Self::LBrace,
        Self::RBrace,
        Self::LBracket,
        Self::RBracket,
        Self::Comma,
        Self::Colon,
        Self::Dot,
        Self::DotDot,
        Self::Semi,
        Self::Question,
        Self::Bang,
        Self::AmpAmp,
        Self::PipePipe,
        Self::ColonColon,
        Self::Arrow,
        Self::FatArrow,
        Self::Eq,
        Self::Plus,
        Self::Minus,
        Self::Star,
        Self::Slash,
        Self::Percent,
        Self::EqEq,
        Self::NotEq,
        Self::Lt,
        Self::LtEq,
        Self::Gt,
        Self::GtEq,
        Self::Amp,
        Self::Pipe,
        Self::Caret,
        Self::Tilde,
        Self::Shl,
        Self::Shr,
        Self::At,
        Self::Whitespace,
        Self::Comment,
        Self::Invalid,
        Self::SourceFile,
        Self::UseItem,
        Self::FnItem,
        Self::RecordItem,
        Self::EnumItem,
        Self::TypeParamList,
        Self::TypeParam,
        Self::ParamList,
        Self::Param,
        Self::RetType,
        Self::EffectSet,
        Self::Contract,
        Self::Type,
        Self::TypeArgList,
        Self::FieldList,
        Self::Field,
        Self::VariantList,
        Self::Variant,
        Self::Block,
        Self::LetStmt,
        Self::AssignStmt,
        Self::Place,
        Self::WhileStmt,
        Self::LetElseStmt,
        Self::ForStmt,
        Self::ReturnStmt,
        Self::BreakStmt,
        Self::ContinueStmt,
        Self::ScopeStmt,
        Self::SpawnStmt,
        Self::ExprStmt,
        Self::Literal,
        Self::NameRef,
        Self::NegExpr,
        Self::NotExpr,
        Self::BitNotExpr,
        Self::ClosureExpr,
        Self::FnType,
        Self::BinaryExpr,
        Self::ParenExpr,
        Self::CallExpr,
        Self::ArgList,
        Self::ConstructExpr,
        Self::VariantExpr,
        Self::FieldInitList,
        Self::FieldInit,
        Self::FieldExpr,
        Self::PropagateExpr,
        Self::IfExpr,
        Self::IfLetExpr,
        Self::MatchExpr,
        Self::MatchArmList,
        Self::MatchArm,
        Self::Pattern,
        Self::OrPattern,
        Self::PatternFieldList,
        Self::PatternField,
        Self::TraitItem,
        Self::TraitMethod,
        Self::ImplItem,
        Self::MethodCallExpr,
        Self::ConstItem,
        Self::ArrayType,
        Self::ArrayExpr,
        Self::IndexExpr,
        Self::Attribute,
        Self::Error,
    ];

    /// The leaf kind of a lexer token, or `None` for [`TokenKind::Eof`], which is a
    /// position and owns no byte, so it is never a leaf.
    ///
    /// Exhaustive and without a wildcard: a token added to the lexer does not compile
    /// until it has a kind here. `IntTooLarge` is an integer literal the parser refuses,
    /// and it is spelled like one, so it is an [`SyntaxKind::Int`] leaf.
    #[must_use]
    pub const fn of_token(kind: &TokenKind) -> Option<Self> {
        Some(match kind {
            TokenKind::Int(_) | TokenKind::IntTooLarge(_) => Self::Int,
            TokenKind::Float(_) => Self::Float,
            TokenKind::Str(_) => Self::Str,
            TokenKind::Ident(_) => Self::Ident,
            TokenKind::Fn => Self::FnKw,
            TokenKind::Let => Self::LetKw,
            TokenKind::If => Self::IfKw,
            TokenKind::Else => Self::ElseKw,
            TokenKind::True => Self::TrueKw,
            TokenKind::False => Self::FalseKw,
            TokenKind::Mut => Self::MutKw,
            TokenKind::Pub => Self::PubKw,
            TokenKind::While => Self::WhileKw,
            TokenKind::Break => Self::BreakKw,
            TokenKind::Continue => Self::ContinueKw,
            TokenKind::Return => Self::ReturnKw,
            TokenKind::Use => Self::UseKw,
            TokenKind::Scope => Self::ScopeKw,
            TokenKind::Spawn => Self::SpawnKw,
            TokenKind::Struct => Self::StructKw,
            TokenKind::Enum => Self::EnumKw,
            TokenKind::Match => Self::MatchKw,
            TokenKind::Trait => Self::TraitKw,
            TokenKind::Impl => Self::ImplKw,
            TokenKind::LParen => Self::LParen,
            TokenKind::RParen => Self::RParen,
            TokenKind::LBrace => Self::LBrace,
            TokenKind::RBrace => Self::RBrace,
            TokenKind::LBracket => Self::LBracket,
            TokenKind::RBracket => Self::RBracket,
            TokenKind::Comma => Self::Comma,
            TokenKind::Colon => Self::Colon,
            TokenKind::Dot => Self::Dot,
            TokenKind::DotDot => Self::DotDot,
            TokenKind::Semi => Self::Semi,
            TokenKind::Question => Self::Question,
            TokenKind::Bang => Self::Bang,
            TokenKind::AmpAmp => Self::AmpAmp,
            TokenKind::ColonColon => Self::ColonColon,
            TokenKind::PipePipe => Self::PipePipe,
            TokenKind::Arrow => Self::Arrow,
            TokenKind::FatArrow => Self::FatArrow,
            TokenKind::Eq => Self::Eq,
            TokenKind::Plus => Self::Plus,
            TokenKind::Minus => Self::Minus,
            TokenKind::Star => Self::Star,
            TokenKind::Slash => Self::Slash,
            TokenKind::Percent => Self::Percent,
            TokenKind::EqEq => Self::EqEq,
            TokenKind::NotEq => Self::NotEq,
            TokenKind::Lt => Self::Lt,
            TokenKind::LtEq => Self::LtEq,
            TokenKind::Gt => Self::Gt,
            TokenKind::GtEq => Self::GtEq,
            TokenKind::Amp => Self::Amp,
            TokenKind::Pipe => Self::Pipe,
            TokenKind::Caret => Self::Caret,
            TokenKind::Tilde => Self::Tilde,
            TokenKind::Shl => Self::Shl,
            TokenKind::Shr => Self::Shr,
            TokenKind::At => Self::At,
            TokenKind::Eof => return None,
        })
    }

    /// A leaf the grammar reads: one of the language's tokens.
    #[must_use]
    pub fn is_token(self) -> bool {
        self <= Self::At
    }

    /// A leaf the grammar skips: whitespace, a comment, or bytes that are not Nazm.
    #[must_use]
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::Comment | Self::Invalid)
    }

    /// A node rather than a leaf.
    #[must_use]
    pub fn is_node(self) -> bool {
        self >= Self::SourceFile
    }
}

impl Syntax for SyntaxKind {
    fn from_raw(raw: RawSyntaxKind) -> Self {
        // Raw values are only ever produced by `into_raw` below, so every one is in the
        // table; `Error` is the answer for one that is not rather than a panic.
        usize::try_from(raw.0)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(Self::Error)
    }

    fn into_raw(self) -> RawSyntaxKind {
        RawSyntaxKind(self as u32)
    }

    /// Always `None`, on purpose.
    ///
    /// A kind with static text is stored without its text and printed back from the kind,
    /// which is reconstruction from canonical spelling — exactly what a lossless tree must
    /// not do. The lexer happens to make `fn` the only spelling of [`SyntaxKind::FnKw`]
    /// today, but "happens to" is a property of today's lexer; the tree's claim is that
    /// every leaf is the bytes it was scanned from. Interning makes the cost of that a
    /// table lookup per repeated spelling.
    fn static_text(self) -> Option<&'static str> {
        None
    }
}

/// A lossless tree of one file.
///
/// One file, not a compilation: a multi-file program has one tree per file, and every
/// span a tree reports names the [`FileId`] it was parsed as.
#[derive(Debug, Clone)]
pub struct Cst {
    root: ResolvedNode<SyntaxKind>,
    file: FileId,
    /// The strings every leaf's key resolves to, kept so a later revision can extend them
    /// and still resolve this tree's leaves (N76).
    strings: Arc<Strings>,
}

/// The leaf texts of one or more trees: an append-only table, so a key handed out once
/// resolves to the same text for as long as any copy of the table lives (N76).
///
/// cstree's own interner cannot be copied, and a reused green node keeps the keys it was
/// built with — so incremental reparse clones this table, extends it with the new
/// region's texts, and resolves old and new leaves through the one extended copy.
#[derive(Debug, Clone, Default)]
pub struct Strings {
    texts: Vec<Box<str>>,
    /// The fast hasher (N106): interning every leaf's text was a tenth of a parse on SipHash.
    index: nazm_span::FastMap<Box<str>, u32>,
}

/// More distinct leaf texts than a [`TokenKey`] can name.
#[derive(Debug, Clone, Copy)]
pub struct TooManyStrings;

impl Resolver<TokenKey> for Strings {
    fn try_resolve(&self, key: TokenKey) -> Option<&str> {
        self.texts
            .get(usize::try_from(key.into_u32()).ok()?)
            .map(|t| &**t)
    }
}

impl Interner<TokenKey> for Strings {
    type Error = TooManyStrings;

    fn try_get_or_intern(&mut self, text: &str) -> Result<TokenKey, Self::Error> {
        if let Some(&k) = self.index.get(text) {
            return TokenKey::try_from_u32(k).ok_or(TooManyStrings);
        }
        let k = u32::try_from(self.texts.len()).map_err(|_| TooManyStrings)?;
        let key = TokenKey::try_from_u32(k).ok_or(TooManyStrings)?;
        self.texts.push(text.into());
        self.index.insert(text.into(), k);
        Ok(key)
    }
}

/// A tree's resolver: the table it shares with the [`Cst`] that owns it.
struct Shared(Arc<Strings>);

impl Resolver<TokenKey> for Shared {
    fn try_resolve(&self, key: TokenKey) -> Option<&str> {
        self.0.try_resolve(key)
    }
}

/// One root-level child of a tree, owned: what incremental reparse moves between trees.
pub(crate) type GreenChild = NodeOrToken<GreenNode, GreenToken>;

/// The result of parsing one file into its concrete tree.
#[derive(Debug, Clone)]
pub struct CstParse {
    pub cst: Cst,
    /// Every lexical and syntactic diagnostic, exactly as [`crate::parse_file`] reports
    /// them for the same text.
    pub diagnostics: Vec<nazm_diag::Diagnostic>,
}

/// One leaf of the tree: its kind, where it is, and the source bytes it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaf<'a> {
    pub kind: SyntaxKind,
    pub span: Span,
    pub text: &'a str,
}

/// One node of the tree. Borrowed from its [`Cst`].
#[derive(Debug, Clone, Copy)]
pub struct CstNode<'a> {
    node: &'a SyntaxNode<SyntaxKind>,
    cst: &'a Cst,
}

impl Cst {
    /// A tree of `file` whose root is `green`, its leaves resolved through `strings`.
    pub(crate) fn from_green(file: FileId, green: GreenNode, strings: Strings) -> Self {
        let strings = Arc::new(strings);
        Self {
            root: SyntaxNode::new_root_with_resolver(green, Shared(Arc::clone(&strings))),
            file,
            strings,
        }
    }

    /// A tree of `file` assembled from root-level children, which may come from other trees
    /// whose keys `strings` extends.
    pub(crate) fn from_children(file: FileId, children: Vec<GreenChild>, strings: Strings) -> Self {
        let green = GreenNode::new(SyntaxKind::SourceFile.into_raw(), children);
        Self::from_green(file, green, strings)
    }

    /// The root's children, owned: cheap, each is a reference-counted green node or token.
    pub(crate) fn root_children(&self) -> Vec<GreenChild> {
        self.root
            .green()
            .children()
            .map(|c| match c {
                NodeOrToken::Node(n) => NodeOrToken::Node(n.clone()),
                NodeOrToken::Token(t) => NodeOrToken::Token(t.clone()),
            })
            .collect()
    }

    /// A copy of the strings this tree's leaves resolve through, for a tree built from it.
    pub(crate) fn strings(&self) -> Strings {
        (*self.strings).clone()
    }

    /// The file this tree is of.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// The root, a [`SyntaxKind::SourceFile`] node spanning the whole file.
    #[must_use]
    pub fn root(&self) -> CstNode<'_> {
        CstNode {
            node: self.root.syntax(),
            cst: self,
        }
    }

    /// The source, rebuilt by concatenating every leaf in order.
    #[must_use]
    pub fn text(&self) -> String {
        self.root().text()
    }

    /// Every leaf, in source order.
    #[must_use]
    pub fn leaves(&self) -> Vec<Leaf<'_>> {
        self.root().leaves()
    }

    /// Every node, in preorder, the root first.
    #[must_use]
    pub fn nodes(&self) -> Vec<CstNode<'_>> {
        self.root().descendants()
    }

    fn span(&self, range: TextRange) -> Span {
        Span::new(self.file, u32::from(range.start()), u32::from(range.end()))
    }
}

impl<'a> CstNode<'a> {
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.node.kind()
    }

    /// From the first byte of its first leaf to the last byte of its last.
    #[must_use]
    pub fn span(&self) -> Span {
        self.cst.span(self.node.text_range())
    }

    /// The node's own leaves, in order.
    #[must_use]
    pub fn leaves(&self) -> Vec<Leaf<'a>> {
        let resolver = self.cst.root.resolver();
        self.node
            .descendants_with_tokens()
            .filter_map(|e| e.as_token().copied())
            .map(|t| Leaf {
                kind: t.kind(),
                span: self.cst.span(t.text_range()),
                text: t.resolve_text(&**resolver),
            })
            .collect()
    }

    /// The leaves that are direct children of this node, not those of its child nodes.
    #[must_use]
    pub fn child_leaves(&self) -> Vec<Leaf<'a>> {
        let resolver = self.cst.root.resolver();
        self.node
            .children_with_tokens()
            .filter_map(|e| e.as_token().copied())
            .map(|t| Leaf {
                kind: t.kind(),
                span: self.cst.span(t.text_range()),
                text: t.resolve_text(&**resolver),
            })
            .collect()
    }

    /// The source bytes this node covers, from its leaves — never from the file.
    #[must_use]
    pub fn text(&self) -> String {
        self.leaves().iter().map(|l| l.text).collect()
    }

    /// The direct child nodes, in order. Leaves are not included.
    #[must_use]
    pub fn children(&self) -> Vec<Self> {
        self.node
            .children()
            .map(|node| Self {
                node,
                cst: self.cst,
            })
            .collect()
    }

    /// This node and every node inside it, in preorder.
    #[must_use]
    pub fn descendants(&self) -> Vec<Self> {
        self.node
            .descendants()
            .map(|node| Self {
                node,
                cst: self.cst,
            })
            .collect()
    }

    /// The smallest node that covers `span` entirely, if `span` is inside this one.
    ///
    /// Found through the tree's structure, not by searching the text: this is the lookup
    /// a structured edit starts from.
    #[must_use]
    pub fn covering(&self, span: Span) -> Option<Self> {
        let range = TextRange::new(span.start.into(), span.end.into());
        if span.file != self.cst.file || !self.node.text_range().contains_range(range) {
            return None;
        }
        let mut at: &'a SyntaxNode<SyntaxKind> = self.node;
        while let Some(c) = at.children().find(|c| c.text_range().contains_range(range)) {
            at = c;
        }
        Some(Self {
            node: at,
            cst: self.cst,
        })
    }
}

/// Where a construct began, for [`Sink::wrap`]: a position in the event list.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Checkpoint(usize);

/// One thing the parser did, in order.
#[derive(Debug, Clone, Copy)]
enum Event {
    /// Consumed the next token.
    Token,
    /// Recognised a construct, which ends here and began at a checkpoint.
    Close,
}

/// Records the tree while the parser runs, and builds it once the parser has finished.
/// Owned by the parser; nothing else drives it.
///
/// The parser reports three things: that it consumed a token, where a construct began (a
/// [`Checkpoint`], which is only a position in the event list), and that a construct that
/// began at a checkpoint has been recognised. Nothing else is recorded — in particular no
/// trivia, and no node is opened until its construct is known to exist, so a construct
/// that fails part way leaves nothing behind but its tokens, for the recovery point above
/// it to wrap in an [`SyntaxKind::Error`].
///
/// # Where trivia goes
///
/// Decided once, when the tree is built: the whitespace, comments and invalid bytes before
/// a token are emitted immediately before that token, **after** every node that closed
/// before it and **before** every node that opens at it. So no node begins or ends with
/// trivia, and trivia between two constructs belongs to the node that contains both.
/// Deciding it while parsing would put it inside whatever construct was being attempted
/// when it was seen, which for a construct that then failed is the wrong one.
pub(crate) struct Sink<'a> {
    text: &'a str,
    lexemes: &'a [Lexeme],
    events: Vec<Event>,
    /// Every recognised construct: the event it begins before, and its kind, in the order
    /// they were recognised.
    opens: Vec<(usize, SyntaxKind)>,
}

impl std::fmt::Debug for Sink<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sink")
            .field("events", &self.events.len())
            .field("nodes", &self.opens.len())
            .finish()
    }
}

impl<'a> Sink<'a> {
    pub(crate) const fn new(text: &'a str, lexemes: &'a [Lexeme]) -> Self {
        Self {
            text,
            lexemes,
            events: Vec::new(),
            opens: Vec::new(),
        }
    }

    /// The parser consumed its next token. Called exactly once for every token it
    /// consumes, and never for [`TokenKind::Eof`].
    pub(crate) fn token(&mut self) {
        self.events.push(Event::Token);
    }

    /// Where the next construct would begin, if it turns out to be one.
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.events.len())
    }

    /// The construct that began at `at` has been recognised, and ends here.
    pub(crate) fn wrap(&mut self, at: Checkpoint, kind: SyntaxKind) {
        self.opens.push((at.0, kind));
        self.events.push(Event::Close);
    }

    /// Build the tree: every lexeme in order, the nodes around them, and — when the parse
    /// reached the end of the file — the trailing trivia last. Returns a `SourceFile` node
    /// and the strings its leaves use, which extend `strings` (N76).
    pub(crate) fn finish_green(self, strings: Strings, at_end: bool) -> (GreenNode, Strings) {
        let Self {
            text,
            lexemes,
            events,
            mut opens,
        } = self;
        // Nodes that begin at the same event nest the other way round from the order
        // they were recognised in: `a - b - c` recognises `a - b` first, and it is the
        // inner one. So each position's nodes open latest-recognised first: ordered by
        // position, and within a position latest first. Positions are event indices, so a
        // counting pass orders them in linear time — which a comparison sort, at a tenth of a
        // parse, did not (N106) — and filling each position's bucket from the last-recognised
        // node down gives the within-position order.
        let positions = events.len() + 1;
        let mut first = vec![0_usize; positions + 1];
        for &(at, _) in &opens {
            if let Some(c) = first.get_mut(at.min(positions - 1) + 1) {
                *c += 1;
            }
        }
        for k in 1..first.len() {
            let before = first.get(k - 1).copied().unwrap_or(0);
            if let Some(c) = first.get_mut(k) {
                *c += before;
            }
        }
        let mut sorted = vec![(0_usize, SyntaxKind::Error); opens.len()];
        for &(at, kind) in opens.iter().rev() {
            if let Some(next) = first.get_mut(at.min(positions - 1)) {
                if let Some(slot) = sorted.get_mut(*next) {
                    *slot = (at, kind);
                }
                *next += 1;
            }
        }
        opens.clear();

        let mut b = Builder {
            text,
            lexemes,
            next: 0,
            builder: GreenNodeBuilder::from_interner(strings),
        };
        b.builder.start_node(SyntaxKind::SourceFile);
        let mut pending = sorted.iter().peekable();
        for (i, event) in events.iter().enumerate() {
            if matches!(event, Event::Token) {
                b.trivia();
            }
            while let Some(&(_, kind)) = pending.next_if(|(at, _)| *at == i) {
                b.builder.start_node(kind);
            }
            match event {
                Event::Token => b.token(),
                Event::Close => b.builder.finish_node(),
            }
        }
        // Every lexeme after the last token: trailing whitespace and comments, and `Eof`,
        // which owns no byte and is not a leaf. A region that stopped before the end owns
        // none of them: they are the leading trivia of the item it stopped at.
        while at_end && b.next < lexemes.len() {
            b.trivia();
            b.next += 1;
        }
        b.builder.finish_node();
        let (green, cache) = b.builder.finish();
        // `from_interner` owns its cache, so it always hands it back.
        let strings = cache
            .and_then(cstree::build::NodeCache::into_interner)
            .unwrap_or_default();
        (green, strings)
    }
}

/// The green-tree builder and the position in the lossless scan, while [`Sink::finish`]
/// replays the events.
struct Builder<'a> {
    text: &'a str,
    lexemes: &'a [Lexeme],
    /// The next lexeme not yet in the tree.
    next: usize,
    builder: GreenNodeBuilder<'static, 'static, SyntaxKind, Strings>,
}

impl Builder<'_> {
    fn leaf(&mut self, kind: SyntaxKind, span: Span) {
        let text = self
            .text
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default();
        self.builder.token(kind, text);
    }

    /// Emit every lexeme before the next token: whitespace, comments, invalid bytes.
    fn trivia(&mut self) {
        while let Some(l) = self.lexemes.get(self.next) {
            let kind = match l {
                Lexeme::Token(_) => return,
                Lexeme::Whitespace(_) => SyntaxKind::Whitespace,
                Lexeme::Comment(_) => SyntaxKind::Comment,
                Lexeme::Invalid(_) => SyntaxKind::Invalid,
            };
            self.leaf(kind, l.span());
            self.next += 1;
        }
    }

    /// Emit the next token.
    fn token(&mut self) {
        if let Some(Lexeme::Token(t)) = self.lexemes.get(self.next) {
            if let Some(kind) = SyntaxKind::of_token(&t.kind) {
                self.leaf(kind, t.span);
            }
            self.next += 1;
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::SyntaxKind;
    use crate::lexer::TokenKind;
    use cstree::Syntax;

    #[test]
    fn a_raw_kind_is_its_index_in_the_table_both_ways() {
        for (i, k) in SyntaxKind::ALL.iter().enumerate() {
            let raw = k.into_raw();
            assert_eq!(usize::try_from(raw.0).ok(), Some(i), "{k:?}");
            assert_eq!(SyntaxKind::from_raw(raw), *k);
        }
        // The table ends at the last variant: nothing declared after `Error` is missing.
        assert_eq!(SyntaxKind::ALL.last(), Some(&SyntaxKind::Error));
    }

    #[test]
    fn every_token_the_lexer_has_is_one_leaf_kind_of_its_own() {
        let mut seen = Vec::new();
        for t in TokenKind::ALL {
            let Some(k) = SyntaxKind::of_token(t) else {
                panic!("{t:?} has no leaf kind");
            };
            assert!(
                k.is_token() && !k.is_trivia() && !k.is_node(),
                "{t:?} -> {k:?}"
            );
            assert!(!seen.contains(&k), "{t:?} shares {k:?} with another token");
            seen.push(k);
        }
        // Every token kind of the tree is some lexer token's: no orphan leaf kinds.
        let tokens = SyntaxKind::ALL.iter().filter(|k| k.is_token()).count();
        assert_eq!(tokens, seen.len());
        assert_eq!(SyntaxKind::of_token(&TokenKind::Eof), None);
        assert_eq!(
            SyntaxKind::of_token(&TokenKind::IntTooLarge(String::new())),
            Some(SyntaxKind::Int)
        );
    }

    #[test]
    fn no_kind_has_a_fixed_spelling() {
        for k in SyntaxKind::ALL {
            assert_eq!(k.static_text(), None, "{k:?}");
        }
    }
}
