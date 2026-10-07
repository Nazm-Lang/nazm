//! Each function's two digests: what it means, and what code generation reads.
//!
//! # The law (`docs/architecture.md` §7.41)
//!
//! BLAKE3 over a canonical encoding of the function's Core IR in which every definition and
//! type is named durably ([`Names`]) and nothing else about the session appears: no span, no
//! display name, no `FnId`, `RecordId` or file index, no map order. Locals and loops are
//! numbered by position in the function, which is a fact about its body.
//!
//! - **semantic**: the signature, the effect contract — declared and required — and the body.
//! - **body**: parameter, local and result types and the body; no effect metadata.
//!
//! So an effect-only change moves `semantic` and not `body`; a capability parameter moves
//! both, because it is a type; a provenance-only change moves neither, because provenance is
//! not in Core IR. Machine-code identity is further down still: the object cache keys on the
//! emitted IR's bytes.
//!
//! # What the encoding normalises
//!
//! Only what the language says means nothing. A field, variant and payload field are encoded
//! by **name**, so moving declarations changes no digest. A `match`'s arms are encoded in
//! variant-name order, because they are exhaustive and disjoint and their written order means
//! nothing. Initialisers stay in written order — that is evaluation order — and so do
//! statements and arguments. Renaming a local changes nothing; its position is its identity.

use nazm_sema::module::ModuleGraph;
use nazm_sema::resolve::Resolution;
use nazm_sema::{EffectSet, Type};

use crate::ir::{Block, Const, Ends, Expr, ExprKind, Function, Match, Program, Stmt};
use crate::names::Names;

/// One function's digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digests {
    pub semantic: [u8; 32],
    pub body: [u8; 32],
}

impl Digests {
    #[must_use]
    pub fn semantic_hex(&self) -> String {
        hex(&self.semantic)
    }

    #[must_use]
    pub fn body_hex(&self) -> String {
        hex(&self.body)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every function's digests, in [`Program::functions`] order.
#[must_use]
pub fn digests(program: &Program, res: &Resolution, graph: &ModuleGraph) -> Vec<Digests> {
    let names = Names::new(res, graph);
    program
        .functions
        .iter()
        .map(|f| function(f, names))
        .collect()
}

/// One function's digests.
#[must_use]
pub fn function(f: &Function, names: Names<'_>) -> Digests {
    let mut body = Canon::new(names, f);
    body.tag("body");
    body.code(f);
    let body = body.finish();

    let mut semantic = Canon::new(names, f);
    semantic.tag("semantic");
    semantic.effects(f.effects.declared);
    semantic.set(f.effects.required);
    semantic.code(f);
    Digests {
        semantic: semantic.finish(),
        body,
    }
}

/// The canonical encoding, written straight into a hasher. Every item is tagged and every
/// variable-length item is length-prefixed, so no two different sequences encode alike.
struct Canon<'a> {
    names: Names<'a>,
    f: &'a Function,
    h: blake3::Hasher,
}

impl<'a> Canon<'a> {
    fn new(names: Names<'a>, f: &'a Function) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(crate::SCHEMA.as_bytes());
        Self { names, f, h }
    }

    fn finish(self) -> [u8; 32] {
        *self.h.finalize().as_bytes()
    }

    fn bytes(&mut self, b: &[u8]) {
        self.h
            .update(&u64::try_from(b.len()).unwrap_or(u64::MAX).to_le_bytes());
        self.h.update(b);
    }

    fn tag(&mut self, t: &str) {
        self.bytes(t.as_bytes());
    }

    fn n(&mut self, n: u64) {
        self.h.update(&n.to_le_bytes());
    }

    fn len(&mut self, n: usize) {
        self.n(u64::try_from(n).unwrap_or(u64::MAX));
    }

    fn ty(&mut self, t: Type) {
        let name = self.names.ty(t);
        self.bytes(name.as_bytes());
    }

    fn opt_ty(&mut self, t: Option<Type>) {
        match t {
            Some(t) => {
                self.tag("value");
                self.ty(t);
            }
            None => self.tag("never"),
        }
    }

    fn set(&mut self, s: EffectSet) {
        self.len(s.names().len());
        for n in s.names() {
            self.tag(n);
        }
    }

    fn effects(&mut self, declared: Option<EffectSet>) {
        match declared {
            Some(s) => {
                self.tag("declared");
                self.set(s);
            }
            None => self.tag("undeclared"),
        }
    }

    /// What code generation reads: the signature's types, every local's type, the body.
    fn code(&mut self, f: &Function) {
        // A foreign function's code is its symbol (N42).
        if let Some(sym) = &f.foreign {
            self.tag("foreign");
            self.tag(sym);
        }
        // An export's C entry is code too (N55).
        if let Some(sym) = &f.export {
            self.tag("export");
            self.tag(sym);
        }
        self.n(u64::from(f.type_params));
        self.n(u64::from(f.params));
        self.ty(f.ret);
        self.len(f.locals.len());
        for l in &f.locals {
            self.ty(l.ty);
        }
        self.block(&f.body);
    }

    fn ends(&mut self, e: Ends) {
        self.tag(match e {
            Ends::FallsThrough => "falls",
            Ends::Transfers => "transfers",
        });
    }

    fn block(&mut self, b: &Block) {
        self.tag("block");
        self.ends(b.ends);
        self.len(b.owns.len());
        for l in &b.owns {
            self.n(u64::from(l.0));
        }
        self.len(b.stmts.len());
        for s in &b.stmts {
            self.stmt(s);
        }
        match &b.tail {
            Some(t) => {
                self.tag("tail");
                self.expr(t);
            }
            None => self.tag("no-tail"),
        }
    }

    fn exprs(&mut self, es: &[Expr]) {
        self.len(es.len());
        for e in es {
            self.expr(e);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { local, value } => {
                self.tag("let");
                self.n(u64::from(local.0));
                self.expr(value);
            }
            Stmt::Assign {
                local, path, value, ..
            } => {
                self.tag("assign");
                self.n(u64::from(local.0));
                self.len(path.len());
                for p in path {
                    match p {
                        crate::ir::PathStep::Field(p) => {
                            self.ty(Type::Record(p.record));
                            let name = self.names.field(p.record, p.field);
                            self.bytes(name.as_bytes());
                        }
                        crate::ir::PathStep::Index { index, len, .. } => {
                            self.tag("index");
                            self.n(u64::from(*len));
                            self.expr(index);
                        }
                    }
                }
                self.expr(value);
            }
            Stmt::Eval(e) => {
                self.tag("eval");
                self.expr(e);
            }
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.tag("if");
                self.expr(cond);
                self.block(then);
                match otherwise {
                    Some(b) => self.block(b),
                    None => self.tag("no-else"),
                }
            }
            Stmt::Match(m) => {
                self.tag("match-stmt");
                self.matching(m);
            }
            Stmt::Loop { id, cond, body, .. } => {
                self.tag("loop");
                self.n(u64::from(id.0));
                self.expr(cond);
                self.block(body);
            }
            Stmt::Block(b) => {
                self.tag("block-stmt");
                self.block(b);
            }
            Stmt::Scope { body, .. } => {
                self.tag("scope");
                self.block(body);
            }
            Stmt::Spawn { callee, args, .. } => {
                self.tag("spawn");
                let name = self.names.function(*callee);
                self.bytes(name.as_bytes());
                self.exprs(args);
            }
            Stmt::Return { value, .. } => {
                self.tag("return");
                self.expr(value);
            }
            Stmt::Break { target, .. } => {
                self.tag("break");
                self.n(u64::from(target.0));
            }
            Stmt::Continue { target, .. } => {
                self.tag("continue");
                self.n(u64::from(target.0));
            }
        }
    }

    fn matching(&mut self, m: &Match) {
        self.tag(match m.origin {
            crate::ir::MatchOrigin::Written => "written",
            crate::ir::MatchOrigin::Propagate => "propagate",
        });
        self.ty(Type::Enum(m.enumeration));
        self.expr(&m.scrutinee);
        // Exhaustive and disjoint: the order they were written in means nothing.
        let mut arms: Vec<(String, &crate::ir::Arm)> = m
            .arms
            .iter()
            .map(|a| (self.names.variant(m.enumeration, a.variant), a))
            .collect();
        arms.sort_by(|a, b| a.0.cmp(&b.0));
        self.len(arms.len());
        for (name, a) in arms {
            self.bytes(name.as_bytes());
            self.len(a.bindings.len());
            for b in &a.bindings {
                let field = self.names.payload(m.enumeration, a.variant, b.field);
                self.bytes(field.as_bytes());
                self.n(u64::from(b.local.0));
            }
            self.block(&a.body);
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.opt_ty(e.ty);
        match &e.kind {
            ExprKind::Const(c) => {
                self.tag("const");
                match self.f.constant(*c) {
                    Some(Const::Int(n)) => {
                        self.tag("int");
                        self.h.update(&n.to_le_bytes());
                    }
                    Some(Const::Bool(b)) => {
                        self.tag("bool");
                        self.h.update(&[u8::from(*b)]);
                    }
                    Some(Const::Str(s)) => {
                        self.tag("str");
                        self.bytes(s);
                    }
                    Some(Const::Num { kind, bits }) => {
                        self.tag(kind.name());
                        self.h.update(&bits.to_le_bytes());
                    }
                    None => self.tag("?"),
                }
            }
            ExprKind::Local(l) => {
                self.tag("local");
                self.n(u64::from(l.0));
            }
            ExprKind::Neg(x) => {
                self.tag("neg");
                self.expr(x);
            }
            ExprKind::Arith { op, lhs, rhs } => {
                self.tag(op.symbol());
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Bits { op, lhs, rhs } => {
                self.tag("bits");
                self.tag(op.symbol());
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Complement(x) => {
                self.tag("complement");
                self.expr(x);
            }
            ExprKind::Convert { mode, operand } => {
                self.tag("convert");
                self.tag(convert_name(*mode));
                self.expr(operand);
            }
            ExprKind::Fits { to, operand } => {
                self.tag("fits");
                self.ty(*to);
                self.expr(operand);
            }
            ExprKind::Array { elems } => {
                self.tag("array");
                self.len(elems.len());
                for e in elems {
                    self.expr(e);
                }
            }
            ExprKind::ArrayRepeat { value, count } => {
                self.tag("array-repeat");
                self.n(u64::from(*count));
                self.expr(value);
            }
            ExprKind::Index { base, index } => {
                self.tag("index");
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Compare {
                op,
                operand,
                law,
                lhs,
                rhs,
            } => {
                self.tag(op.symbol());
                self.ty(*operand);
                self.tag(law.name());
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                self.tag("call");
                let name = self.names.function(*callee);
                self.bytes(name.as_bytes());
                self.len(type_args.len());
                for t in type_args {
                    self.ty(*t);
                }
                self.exprs(args);
            }
            ExprKind::Builtin {
                op,
                type_args,
                args,
                ..
            } => {
                self.tag("builtin");
                self.tag(op.name());
                self.len(type_args.len());
                for t in type_args {
                    self.ty(*t);
                }
                self.exprs(args);
            }
            ExprKind::Record { record, fields, .. } => {
                self.tag("record");
                self.ty(Type::Record(*record));
                self.len(fields.len());
                for i in fields {
                    let name = self.names.field(*record, i.field);
                    self.bytes(name.as_bytes());
                    self.expr(&i.value);
                }
            }
            ExprKind::Variant {
                enumeration,
                variant,
                fields,
                ..
            } => {
                self.tag("variant");
                self.ty(Type::Enum(*enumeration));
                let name = self.names.variant(*enumeration, *variant);
                self.bytes(name.as_bytes());
                self.len(fields.len());
                for i in fields {
                    let name = self.names.payload(*enumeration, *variant, i.field);
                    self.bytes(name.as_bytes());
                    self.expr(&i.value);
                }
            }
            ExprKind::Field {
                base,
                record,
                field,
            } => {
                self.tag("field");
                self.ty(Type::Record(*record));
                let name = self.names.field(*record, *field);
                self.bytes(name.as_bytes());
                self.expr(base);
            }
            ExprKind::Match(m) => {
                self.tag("match");
                self.matching(m);
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.tag("if-value");
                self.expr(cond);
                self.block(then);
                self.block(otherwise);
            }
            ExprKind::Block(b) => {
                self.tag("block-value");
                self.block(b);
            }
            // N50. A closure names its lifted body durably, as a call does, and the locals it
            // copies; a function value names its function; an indirect call is its callee
            // and its arguments.
            ExprKind::Closure { func, captures } => {
                self.tag("closure");
                let name = self.names.function(*func);
                self.bytes(name.as_bytes());
                self.len(captures.len());
                for l in captures {
                    self.n(u64::from(l.0));
                }
            }
            ExprKind::FnValue(func) => {
                self.tag("fn-value");
                let name = self.names.function(*func);
                self.bytes(name.as_bytes());
            }
            ExprKind::CallIndirect { callee, args, .. } => {
                self.tag("call-indirect");
                self.expr(callee);
                self.exprs(args);
            }
            ExprKind::CallTrait {
                trait_id,
                method,
                self_ty,
                args,
                ..
            } => {
                self.tag("call-trait");
                let name = self.names.trait_method(*trait_id, *method);
                self.bytes(name.as_bytes());
                self.ty(*self_ty);
                self.exprs(args);
            }
        }
    }
}

/// A conversion mode's name, as the digest and the printed IR write it.
#[must_use]
pub const fn convert_name(mode: nazm_sema::ConvertMode) -> &'static str {
    match mode {
        nazm_sema::ConvertMode::Exact => "exact",
        nazm_sema::ConvertMode::Try => "try",
        nazm_sema::ConvertMode::Wrap => "wrap",
        nazm_sema::ConvertMode::Saturate => "saturate",
    }
}
