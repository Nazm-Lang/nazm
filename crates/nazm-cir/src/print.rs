//! The deterministic printed form, `nazm.core-ir/1`: JSON, for tests, tools and a compiler
//! developer asking what a function became.
//!
//! Not user-facing syntax and not a stable interchange format: the version names what this
//! build writes, and no reader outside the workspace is promised a future one reads the same.
//! What *is* promised is determinism — the same checked program prints the same bytes — and
//! durability: every definition and type is named by its key, so the text does not depend on
//! the checkout or on load-order numbering (a module with no key excepted, as [`Names`] says).
//!
//! Written by hand rather than through `serde`, for the reason `nazm-sema` has none: the
//! values being printed are session-local, and a derive would print their numbers.

use nazm_sema::module::ModuleGraph;
use nazm_sema::resolve::Resolution;
use nazm_sema::EffectSet;
use nazm_span::Span;

use crate::digest::digests;
use crate::ir::{
    Arm, Block, Const, Ends, Expr, ExprKind, Function, LocalOrigin, Match, MatchOrigin, Program,
    Stmt,
};
use crate::names::Names;
use crate::SCHEMA;

/// What to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Include every node's source span. Off, two programs that differ only in where their
    /// text sits print the same.
    pub spans: bool,
    /// One line, the canonical compact form; otherwise indented.
    pub compact: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            spans: true,
            compact: false,
        }
    }
}

/// `program` as `nazm.core-ir/1` text.
#[must_use]
pub fn print(program: &Program, res: &Resolution, graph: &ModuleGraph, options: Options) -> String {
    let names = Names::new(res, graph);
    let p = Printer {
        names,
        spans: options.spans,
    };
    let sums = digests(program, res, graph);
    let functions: Vec<J> = program
        .functions
        .iter()
        .zip(&sums)
        .map(|(f, d)| p.function(f, &d.semantic_hex(), &d.body_hex()))
        .collect();
    let root = J::Obj(vec![
        ("schema", J::Str(SCHEMA.to_owned())),
        (
            "entry",
            program.entry.map_or(J::Null, |e| J::Str(names.function(e))),
        ),
        ("functions", J::Arr(functions)),
    ]);
    let mut out = String::new();
    root.write(&mut out, if options.compact { None } else { Some(0) });
    if !options.compact {
        out.push('\n');
    }
    out
}

/// A JSON value, built in the order it is written.
#[derive(Debug)]
enum J {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl J {
    fn uint(n: impl Into<u64>) -> Self {
        Self::Int(i64::try_from(n.into()).unwrap_or(i64::MAX))
    }

    /// `level` is the indentation of the line this value starts on, or `None` for compact.
    fn write(&self, out: &mut String, level: Option<usize>) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Int(n) => out.push_str(&n.to_string()),
            Self::Str(s) => quote(out, s),
            Self::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, level.map(|l| l + 1));
                    item.write(out, level.map(|l| l + 1));
                }
                newline(out, level);
                out.push(']');
            }
            Self::Obj(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, level.map(|l| l + 1));
                    quote(out, k);
                    out.push(':');
                    if level.is_some() {
                        out.push(' ');
                    }
                    v.write(out, level.map(|l| l + 1));
                }
                newline(out, level);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, level: Option<usize>) {
    if let Some(l) = level {
        out.push('\n');
        for _ in 0..l {
            out.push_str("  ");
        }
    }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Printer<'a> {
    names: Names<'a>,
    spans: bool,
}

fn effects(set: EffectSet) -> J {
    J::Arr(
        set.names()
            .into_iter()
            .map(|n| J::Str(n.to_owned()))
            .collect(),
    )
}

fn ends(e: Ends) -> J {
    J::Str(
        match e {
            Ends::FallsThrough => "falls-through",
            Ends::Transfers => "transfers",
        }
        .to_owned(),
    )
}

impl Printer<'_> {
    fn at(&self, fields: &mut Vec<(&'static str, J)>, s: Span) {
        if self.spans {
            fields.push(("at", J::Str(self.names.span(s))));
        }
    }

    fn ty(&self, t: Option<nazm_sema::Type>) -> J {
        t.map_or_else(|| J::Str("never".to_owned()), |t| J::Str(self.names.ty(t)))
    }

    fn function(&self, f: &Function, semantic: &str, body: &str) -> J {
        let locals: Vec<J> = f
            .locals
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let origin = match l.origin {
                    LocalOrigin::Param => "param".to_owned(),
                    LocalOrigin::Binding { mutable: false } => "let".to_owned(),
                    LocalOrigin::Binding { mutable: true } => "let mut".to_owned(),
                    LocalOrigin::Temp(t) => format!("temp {}", t.name()),
                };
                let mut o = vec![
                    ("id", J::uint(u64::try_from(i).unwrap_or(u64::MAX))),
                    ("name", J::Str(l.name.clone())),
                    ("type", J::Str(self.names.ty(l.ty))),
                    ("origin", J::Str(origin)),
                ];
                self.at(&mut o, l.at);
                J::Obj(o)
            })
            .collect();
        let consts: Vec<J> = f
            .consts
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (ty, value) = match c {
                    Const::Int(n) => ("Int", J::Int(*n)),
                    Const::Bool(b) => ("Bool", J::Bool(*b)),
                    Const::Str(bytes) => ("Str", J::Str(text(bytes))),
                    Const::Num { kind, bits } => (
                        kind.name(),
                        J::Str(nazm_sema::numeric::to_str(nazm_sema::numeric::NumVal {
                            kind: *kind,
                            bits: *bits,
                        })),
                    ),
                };
                J::Obj(vec![
                    ("id", J::uint(u64::try_from(i).unwrap_or(u64::MAX))),
                    ("type", J::Str(ty.to_owned())),
                    ("value", value),
                ])
            })
            .collect();
        let mut o = vec![
            ("def", J::Str(self.names.function(f.def))),
            ("module", J::Str(self.names.module(f.module))),
            ("name", J::Str(f.name.clone())),
            ("type_params", J::uint(f.type_params)),
            ("params", J::uint(f.params)),
            ("ret", J::Str(self.names.ty(f.ret))),
            (
                "effects",
                J::Obj(vec![
                    ("declared", f.effects.declared.map_or(J::Null, effects)),
                    ("required", effects(f.effects.required)),
                    ("published", effects(f.effects.published())),
                    ("pure", J::Bool(f.effects.is_pure())),
                ]),
            ),
            (
                "authority",
                J::Arr(
                    f.authority()
                        .iter()
                        .map(|k| J::Str(k.type_name().to_owned()))
                        .collect(),
                ),
            ),
            (
                "digest",
                J::Obj(vec![
                    ("semantic", J::Str(semantic.to_owned())),
                    ("body", J::Str(body.to_owned())),
                ]),
            ),
            ("locals", J::Arr(locals)),
            ("consts", J::Arr(consts)),
            ("loops", J::uint(f.loops)),
        ];
        self.at(&mut o, f.span);
        match &f.foreign {
            // A foreign function has no body to print, only the C symbol it names (N42).
            Some(sym) => o.push(("foreign", J::Str(sym.clone()))),
            None => o.push(("body", self.block(&f.body))),
        }
        J::Obj(o)
    }

    fn block(&self, b: &Block) -> J {
        let mut o = vec![
            ("ends", ends(b.ends)),
            (
                "owns",
                J::Arr(b.owns.iter().map(|l| J::uint(l.0)).collect()),
            ),
            (
                "stmts",
                J::Arr(b.stmts.iter().map(|s| self.stmt(s)).collect()),
            ),
            ("tail", b.tail.as_ref().map_or(J::Null, |t| self.expr(t))),
        ];
        self.at(&mut o, b.at);
        J::Obj(o)
    }

    fn op(name: &str) -> (&'static str, J) {
        ("op", J::Str(name.to_owned()))
    }

    fn stmt(&self, s: &Stmt) -> J {
        let mut o = Vec::new();
        let at = match s {
            Stmt::Let { local, value } => {
                o.push(Self::op("let"));
                o.push(("local", J::uint(local.0)));
                o.push(("value", self.expr(value)));
                None
            }
            Stmt::Assign {
                local,
                path,
                value,
                at,
            } => {
                o.push(Self::op("assign"));
                o.push(("local", J::uint(local.0)));
                o.push((
                    "path",
                    J::Arr(
                        path.iter()
                            .map(|p| {
                                J::Obj(vec![
                                    (
                                        "record",
                                        J::Str(self.names.ty(nazm_sema::Type::Record(p.record))),
                                    ),
                                    ("field", J::uint(p.field)),
                                    ("name", J::Str(self.names.field(p.record, p.field))),
                                ])
                            })
                            .collect(),
                    ),
                ));
                o.push(("value", self.expr(value)));
                Some(*at)
            }
            Stmt::Eval(e) => {
                o.push(Self::op("eval"));
                o.push(("value", self.expr(e)));
                None
            }
            Stmt::If {
                cond,
                then,
                otherwise,
                at,
            } => {
                o.push(Self::op("if"));
                o.push(("cond", self.expr(cond)));
                o.push(("then", self.block(then)));
                o.push((
                    "else",
                    otherwise.as_ref().map_or(J::Null, |b| self.block(b)),
                ));
                Some(*at)
            }
            Stmt::Match(m) => {
                o.push(Self::op("match"));
                self.match_fields(&mut o, m);
                Some(m.at)
            }
            Stmt::Loop { id, cond, body, at } => {
                o.push(Self::op("loop"));
                o.push(("loop", J::uint(id.0)));
                o.push(("cond", self.expr(cond)));
                o.push(("body", self.block(body)));
                Some(*at)
            }
            Stmt::Block(b) => {
                o.push(Self::op("block"));
                o.push(("body", self.block(b)));
                None
            }
            Stmt::Scope { body, at } => {
                o.push(Self::op("scope"));
                o.push(("body", self.block(body)));
                Some(*at)
            }
            Stmt::Spawn {
                callee, args, at, ..
            } => {
                o.push(Self::op("spawn"));
                o.push(("callee", J::Str(self.names.function(*callee))));
                o.push(("args", J::Arr(args.iter().map(|a| self.expr(a)).collect())));
                Some(*at)
            }
            Stmt::Return { value, at } => {
                o.push(Self::op("return"));
                o.push(("value", self.expr(value)));
                Some(*at)
            }
            Stmt::Break { target, at } => {
                o.push(Self::op("break"));
                o.push(("loop", J::uint(target.0)));
                Some(*at)
            }
            Stmt::Continue { target, at } => {
                o.push(Self::op("continue"));
                o.push(("loop", J::uint(target.0)));
                Some(*at)
            }
        };
        o.push(("ends", ends(s.ends())));
        if let Some(at) = at {
            self.at(&mut o, at);
        }
        J::Obj(o)
    }

    fn match_fields(&self, o: &mut Vec<(&'static str, J)>, m: &Match) {
        o.push((
            "origin",
            J::Str(
                match m.origin {
                    MatchOrigin::Written => "written",
                    MatchOrigin::Propagate => "propagate",
                }
                .to_owned(),
            ),
        ));
        o.push((
            "enum",
            J::Str(self.names.ty(nazm_sema::Type::Enum(m.enumeration))),
        ));
        o.push(("scrutinee", self.expr(&m.scrutinee)));
        o.push((
            "arms",
            J::Arr(m.arms.iter().map(|a| self.arm(m, a)).collect()),
        ));
    }

    fn arm(&self, m: &Match, a: &Arm) -> J {
        let mut o = vec![
            ("variant", J::uint(a.variant)),
            ("name", J::Str(self.names.variant(m.enumeration, a.variant))),
            (
                "bindings",
                J::Arr(
                    a.bindings
                        .iter()
                        .map(|b| {
                            J::Obj(vec![
                                ("field", J::uint(b.field)),
                                (
                                    "name",
                                    J::Str(self.names.payload(m.enumeration, a.variant, b.field)),
                                ),
                                ("local", J::uint(b.local.0)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("body", self.block(&a.body)),
        ];
        self.at(&mut o, a.at);
        J::Obj(o)
    }

    fn expr(&self, e: &Expr) -> J {
        let mut o = Vec::new();
        match &e.kind {
            ExprKind::Const(c) => {
                o.push(Self::op("const"));
                o.push(("const", J::uint(c.0)));
            }
            ExprKind::Local(l) => {
                o.push(Self::op("local"));
                o.push(("local", J::uint(l.0)));
            }
            ExprKind::Neg(x) => {
                o.push(Self::op("neg"));
                o.push(("operand", self.expr(x)));
            }
            ExprKind::Arith { op, lhs, rhs } => {
                o.push(Self::op(op.symbol()));
                o.push(("lhs", self.expr(lhs)));
                o.push(("rhs", self.expr(rhs)));
            }
            ExprKind::Bits { op, lhs, rhs } => {
                o.push(Self::op(op.symbol()));
                o.push(("lhs", self.expr(lhs)));
                o.push(("rhs", self.expr(rhs)));
            }
            ExprKind::Complement(x) => {
                o.push(Self::op("~"));
                o.push(("operand", self.expr(x)));
            }
            ExprKind::Convert { mode, operand } => {
                o.push(Self::op("convert"));
                o.push((
                    "mode",
                    J::Str(crate::digest::convert_name(*mode).to_owned()),
                ));
                o.push(("operand", self.expr(operand)));
            }
            ExprKind::Fits { to, operand } => {
                o.push(Self::op("fits"));
                o.push(("to", J::Str(self.names.ty(*to))));
                o.push(("operand", self.expr(operand)));
            }
            ExprKind::Compare {
                op,
                operand,
                law,
                lhs,
                rhs,
            } => {
                o.push(Self::op(op.symbol()));
                o.push(("operand", J::Str(self.names.ty(*operand))));
                o.push(("law", J::Str(law.name().to_owned())));
                o.push(("lhs", self.expr(lhs)));
                o.push(("rhs", self.expr(rhs)));
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                o.push(Self::op("call"));
                o.push(("callee", J::Str(self.names.function(*callee))));
                o.push(("type_args", self.types(type_args)));
                o.push(("args", J::Arr(args.iter().map(|a| self.expr(a)).collect())));
            }
            ExprKind::Builtin {
                op,
                type_args,
                args,
                ..
            } => {
                o.push(Self::op("builtin"));
                o.push(("builtin", J::Str(op.name().to_owned())));
                o.push(("type_args", self.types(type_args)));
                o.push(("args", J::Arr(args.iter().map(|a| self.expr(a)).collect())));
            }
            ExprKind::Record {
                record,
                arity,
                fields,
            } => {
                o.push(Self::op("record"));
                o.push(("arity", J::uint(*arity)));
                o.push((
                    "fields",
                    J::Arr(
                        fields
                            .iter()
                            .map(|i| {
                                J::Obj(vec![
                                    ("field", J::uint(i.field)),
                                    ("name", J::Str(self.names.field(*record, i.field))),
                                    ("value", self.expr(&i.value)),
                                ])
                            })
                            .collect(),
                    ),
                ));
            }
            ExprKind::Variant {
                enumeration,
                variant,
                arity,
                fields,
            } => {
                o.push(Self::op("variant"));
                o.push(("variant", J::uint(*variant)));
                o.push(("name", J::Str(self.names.variant(*enumeration, *variant))));
                o.push(("arity", J::uint(*arity)));
                o.push((
                    "fields",
                    J::Arr(
                        fields
                            .iter()
                            .map(|i| {
                                J::Obj(vec![
                                    ("field", J::uint(i.field)),
                                    (
                                        "name",
                                        J::Str(self.names.payload(*enumeration, *variant, i.field)),
                                    ),
                                    ("value", self.expr(&i.value)),
                                ])
                            })
                            .collect(),
                    ),
                ));
            }
            ExprKind::Field {
                base,
                record,
                field,
            } => {
                o.push(Self::op("field"));
                o.push(("field", J::uint(*field)));
                o.push(("name", J::Str(self.names.field(*record, *field))));
                o.push(("base", self.expr(base)));
            }
            ExprKind::Match(m) => {
                o.push(Self::op("match"));
                self.match_fields(&mut o, m);
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                o.push(Self::op("if"));
                o.push(("cond", self.expr(cond)));
                o.push(("then", self.block(then)));
                o.push(("else", self.block(otherwise)));
            }
            ExprKind::Block(b) => {
                o.push(Self::op("block"));
                o.push(("body", self.block(b)));
            }
            ExprKind::Closure { func, captures } => {
                o.push(Self::op("closure"));
                o.push(("function", J::Str(self.names.function(*func))));
                o.push((
                    "captures",
                    J::Arr(captures.iter().map(|l| J::uint(l.0)).collect()),
                ));
            }
            ExprKind::FnValue(func) => {
                o.push(Self::op("fn_value"));
                o.push(("function", J::Str(self.names.function(*func))));
            }
            ExprKind::CallIndirect { callee, args, .. } => {
                o.push(Self::op("call_indirect"));
                o.push(("callee", self.expr(callee)));
                o.push(("args", J::Arr(args.iter().map(|a| self.expr(a)).collect())));
            }
            ExprKind::CallTrait {
                trait_id,
                method,
                self_ty,
                args,
                ..
            } => {
                o.push(Self::op("call_trait"));
                o.push((
                    "method",
                    J::Str(self.names.trait_method(*trait_id, *method)),
                ));
                o.push(("self", J::Str(self.names.ty(*self_ty))));
                o.push(("args", J::Arr(args.iter().map(|a| self.expr(a)).collect())));
            }
        }
        o.push(("type", self.ty(e.ty)));
        self.at(&mut o, e.at);
        J::Obj(o)
    }

    fn types(&self, ts: &[nazm_sema::Type]) -> J {
        J::Arr(ts.iter().map(|t| J::Str(self.names.ty(*t))).collect())
    }
}

/// A literal's bytes as text where they are UTF-8, and otherwise as `\xNN` escapes: one
/// spelling per byte sequence either way.
fn text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) if !s.contains("\\x") => s.to_owned(),
        _ => bytes.iter().map(|b| format!("\\x{b:02x}")).collect(),
    }
}
