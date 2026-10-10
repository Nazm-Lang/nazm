//! Constants (Gate 2, `docs/spec.md`, *Constants*): `const NAME: T = value;`, computed by the
//! checker.
//!
//! A constant's value is an expression of literals, other constants of its module, the
//! operators, and numeric conversions `T(x)` — and nothing else: no call, no binding, no
//! effect. It is computed here, once, by the numeric semantics every implementation shares
//! (`nazm_sema::numeric`), so a constant means in a compiled program exactly what it means to
//! the checker. A value that would trap — an overflow, a zero divisor, a shift out of range —
//! is refused where the constant is declared. Constants may name each other in any order; a
//! constant that reaches itself is refused.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
pub use nazm_sema::numeric::ConstValue;
use nazm_sema::numeric::{self, NumError, NumVal};
use nazm_sema::{ConvertMode, NumKind, Type};
use nazm_syntax::{BinOp, Const, Expr};

/// One module's constants: each declared one's type and computed value.
#[derive(Debug, Default, Clone)]
pub struct Constants {
    pub values: HashMap<String, (Type, ConstValue)>,
    /// Where each constant is declared, by its name: the first declaration's name.
    pub decls: HashMap<String, nazm_span::Span>,
    /// Every constant named in another's value, by the name's span (Gate 2): references the
    /// index records, so renaming a constant reaches them.
    pub refs: Vec<(nazm_span::Span, String)>,
    /// The names declared `static` (Gate 3), whatever their type: storage, which no array's
    /// length is.
    pub statics: std::collections::HashSet<String>,
    /// Each static `Atomic` (Gate 3): the value its cell starts with. Not in [`Self::values`]:
    /// a use of one is the cell, never a value written in its place.
    pub cells: HashMap<String, i64>,
    /// Each static's `@section` (Gate 3), by its name.
    pub sections: HashMap<String, String>,
}

/// Compute every constant of a module, refusing what cannot be computed.
pub fn evaluate(decls: &[&Const], errors: &mut Vec<Diagnostic>) -> Constants {
    let by_name: HashMap<&str, &Const> = decls.iter().map(|c| (c.name.as_str(), *c)).collect();
    let mut decl_spans: HashMap<String, nazm_span::Span> = HashMap::new();
    for c in decls {
        decl_spans.entry(c.name.clone()).or_insert(c.name_span);
    }
    let mut ev = Evaluator {
        decls: by_name,
        done: HashMap::new(),
        cells: HashMap::new(),
        active: Vec::new(),
        refs: Vec::new(),
        errors,
    };
    for c in decls {
        if let Some(prior) = decls
            .iter()
            .find(|d| d.name == c.name && d.name_span != c.name_span)
        {
            if prior.name_span.start < c.name_span.start {
                ev.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FUNCTION,
                        c.name_span,
                        format!("`{}` is declared twice in this module", c.name),
                    )
                    .secondary(prior.name_span, "first declared here"),
                );
                continue;
            }
        }
        if let Some(p) = c.pub_span {
            let what = if c.is_static { "static" } else { "constant" };
            ev.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    p,
                    format!(
                        "`{}` cannot be exported: a {what} is its own module's",
                        c.name
                    ),
                )
                .help(
                    "an importer would need the value in the module's published interface, \
                     which does not carry constants yet; export a function returning it",
                ),
            );
        }
        if c.is_static && is_atomic(&c.ty) {
            ev.cell(c);
        } else {
            ev.constant(&c.name);
        }
    }
    let mut refs = ev.refs;
    refs.sort_by_key(|(s, _)| (s.file, s.start));
    refs.dedup();
    Constants {
        values: ev.done,
        decls: decl_spans,
        refs,
        statics: decls
            .iter()
            .filter(|c| c.is_static)
            .map(|c| c.name.clone())
            .collect(),
        cells: ev.cells,
        sections: decls
            .iter()
            .filter(|c| c.is_static)
            .filter_map(|c| section_of(&c.attrs).map(|s| (c.name.clone(), s)))
            .collect(),
    }
}

/// The name a declaration's `@section` gives (Gate 3), held to the attribute's rule elsewhere.
#[must_use]
pub fn section_of(attrs: &[nazm_syntax::ast::Attribute]) -> Option<String> {
    attrs
        .iter()
        .find(|a| a.name == "section")
        .and_then(|a| match a.args.as_deref() {
            Some([nazm_syntax::ast::AttrArg::Str { value, .. }]) => Some(value.clone()),
            _ => None,
        })
}

/// Whether a declared type is `Atomic`, written plainly.
fn is_atomic(t: &nazm_syntax::TypeName) -> bool {
    t.args.is_none() && t.function.is_none() && t.qualifier.is_none() && t.name == "Atomic"
}

struct Evaluator<'a, 'e> {
    decls: HashMap<&'a str, &'a Const>,
    done: HashMap<String, (Type, ConstValue)>,
    cells: HashMap<String, i64>,
    active: Vec<String>,
    refs: Vec<(nazm_span::Span, String)>,
    errors: &'e mut Vec<Diagnostic>,
}

/// Why an expression has no constant value: already reported.
struct Refused;

impl Evaluator<'_, '_> {
    /// A static `Atomic` (Gate 3): its value is `atomic_new(c)` of a constant `Int`, the one call
    /// a static's value may make.
    fn cell(&mut self, decl: &Const) {
        let init = match &decl.value {
            Expr::Call {
                qualifier: None,
                callee,
                args,
                type_args: None,
                ..
            } if callee == "atomic_new" && args.len() == 1 => args.first().and_then(|a| {
                self.active.push(decl.name.clone());
                let v = self.value(a, Type::Int).ok();
                self.active.pop();
                match v {
                    Some(ConstValue::Num(n)) if n.kind == NumKind::I64 =>
                    {
                        #[allow(clippy::cast_possible_wrap)]
                        Some(n.bits as i64)
                    }
                    Some(_) => {
                        self.errors.push(Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            a.span(),
                            "an atomic holds an `Int`",
                        ));
                        None
                    }
                    None => None,
                }
            }),
            other => {
                self.errors.push(
                    Diagnostic::error(
                        codes::NOT_A_CONSTANT,
                        other.span(),
                        "a static `Atomic` starts as `atomic_new(c)` of a constant `c`",
                    )
                    .help("`atomic_new` is the one call a static's value may make"),
                );
                None
            }
        };
        // A cell whose start could not be computed is still declared, so a use of it is not a
        // second error.
        self.cells.insert(decl.name.clone(), init.unwrap_or(0));
    }

    fn constant(&mut self, name: &str) -> Option<(Type, ConstValue)> {
        if let Some(v) = self.done.get(name) {
            return Some(v.clone());
        }
        let decl = *self.decls.get(name)?;
        if self.active.iter().any(|n| n == name) {
            self.errors.push(Diagnostic::error(
                codes::NOT_A_CONSTANT,
                decl.name_span,
                format!("`{name}`'s value depends on itself"),
            ));
            return None;
        }
        let ty = declared(&decl.ty).filter(|t| !(decl.is_static && *t == Type::Str));
        if decl.is_static && ty.is_none() {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_A_CONSTANT,
                    decl.ty.span,
                    format!(
                        "a static is a number, a `Bool` or an `Atomic`, and `{}` is none",
                        decl.ty.name
                    ),
                )
                .help("a static is storage with one address; a `Str` is a constant's"),
            );
            return None;
        }
        let Some(ty) = ty else {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_A_CONSTANT,
                    decl.ty.span,
                    format!(
                        "a constant is a number, a `Bool` or a `Str`, and `{}` is none",
                        decl.ty.name
                    ),
                )
                .help("its value is computed when the program is checked, so it is a value with no storage of its own"),
            );
            return None;
        };
        self.active.push(name.to_owned());
        let value = self.value(&decl.value, ty).ok();
        self.active.pop();
        // A constant that could not be computed is still declared, at its type, so a use of it
        // is not a second error: one mistake is one error.
        let Some(value) = value else {
            self.done.insert(name.to_owned(), (ty, placeholder(ty)));
            return None;
        };
        if value.ty() != ty {
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                decl.value.span(),
                format!(
                    "`{name}` is declared `{}`, and this is `{}`",
                    show(ty),
                    show(value.ty())
                ),
            ));
            return None;
        }
        self.done.insert(name.to_owned(), (ty, value.clone()));
        Some((ty, value))
    }

    fn refuse(&mut self, at: nazm_span::Span, why: &str) -> Refused {
        self.errors.push(
            Diagnostic::error(
                codes::NOT_A_CONSTANT,
                at,
                format!("a constant's value cannot {why}"),
            )
            .help(
                "a constant is literals, other constants, operators and numeric conversions `T(x)`",
            ),
        );
        Refused
    }

    fn fails(&mut self, at: nazm_span::Span, e: NumError) -> Refused {
        let what = match e {
            NumError::Overflow => "overflows its type",
            NumError::DivideByZero => "divides by zero",
            NumError::ShiftRange => "shifts out of range",
        };
        self.errors.push(Diagnostic::error(
            codes::CONSTANT_FAILS,
            at,
            format!("this constant {what}"),
        ));
        Refused
    }

    /// `e`'s value, as a value of `expected` where that decides a literal's type.
    fn value(&mut self, e: &Expr, expected: Type) -> Result<ConstValue, Refused> {
        let want = expected.num();
        match e {
            Expr::Int { value, span } => self.integer(i128::from(*value), want, *span),
            Expr::WideInt { value, span } => self.integer(i128::from(*value), want, *span),
            Expr::Float { text, .. } => {
                let k = if want == Some(NumKind::F32) {
                    NumKind::F32
                } else {
                    NumKind::F64
                };
                Ok(ConstValue::Num(numeric::literal(k, text)))
            }
            Expr::Bool { value, .. } => Ok(ConstValue::Bool(*value)),
            Expr::Str { value, .. } => Ok(ConstValue::Str(value.clone().into_bytes())),
            Expr::Var { name, span } => {
                if self.decls.contains_key(name.as_str()) {
                    self.refs.push((*span, name.clone()));
                }
                // A static atomic's cell changes as the program runs: no constant is made of it.
                if self
                    .decls
                    .get(name.as_str())
                    .is_some_and(|d| d.is_static && is_atomic(&d.ty))
                {
                    return Err(self.refuse(*span, &format!("read the atomic `{name}`")));
                }
                match self.constant(name) {
                    Some((_, v)) => Ok(v),
                    None if self.decls.contains_key(name.as_str()) => Err(Refused),
                    None => {
                        Err(self.refuse(*span, &format!("name `{name}`, which is not a constant")))
                    }
                }
            }
            Expr::Neg { operand, span, .. } => {
                if let Expr::Int { value, span: lit } = operand.as_ref() {
                    if want.is_some_and(|k| k != NumKind::I64 && k.is_integer()) {
                        return self.integer(-i128::from(*value), want, *lit);
                    }
                }
                match self.value(operand, expected)? {
                    ConstValue::Num(n) => numeric::neg(n)
                        .map(ConstValue::Num)
                        .map_err(|err| self.fails(*span, err)),
                    _ => Err(self.refuse(*span, "negate something that is not a number")),
                }
            }
            Expr::BitNot { operand, span, .. } => match self.value(operand, expected)? {
                ConstValue::Num(n) if n.kind.is_integer() => {
                    Ok(ConstValue::Num(numeric::complement(n)))
                }
                _ => Err(self.refuse(*span, "complement something that is not an integer")),
            },
            Expr::Not { operand, span, .. } => match self.value(operand, Type::Bool)? {
                ConstValue::Bool(b) => Ok(ConstValue::Bool(!b)),
                _ => Err(self.refuse(*span, "negate something that is not a `Bool`")),
            },
            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => self.binary(*op, lhs, rhs, *op_span, expected),
            Expr::Call {
                qualifier: None,
                callee,
                args,
                type_args: None,
                span,
                ..
            } if NumKind::by_name(callee).is_some() && args.len() == 1 => {
                let to = NumKind::by_name(callee).unwrap_or(NumKind::I64);
                let Some(a) = args.first() else {
                    return Err(Refused);
                };
                match self.value(a, Type::of_num(to))? {
                    ConstValue::Num(n) => numeric::convert(to, ConvertMode::Exact, n)
                        .map(ConstValue::Num)
                        .map_err(|err| self.fails(*span, err)),
                    _ => Err(self.refuse(*span, "convert something that is not a number")),
                }
            }
            other => Err(self.refuse(other.span(), "be computed from this")),
        }
    }

    fn integer(
        &mut self,
        v: i128,
        want: Option<NumKind>,
        at: nazm_span::Span,
    ) -> Result<ConstValue, Refused> {
        let k = want.filter(|k| k.is_integer()).unwrap_or(NumKind::I64);
        match NumVal::int(k, v) {
            Some(n) => Ok(ConstValue::Num(n)),
            None => {
                let (lo, hi) = k.int_range().unwrap_or((0, 0));
                self.errors.push(
                    Diagnostic::error(
                        codes::INTEGER_TOO_LARGE,
                        at,
                        format!("`{v}` does not fit in `{}`", k.name()),
                    )
                    .help(format!("`{}` holds {lo} to {hi}", k.name())),
                );
                Err(Refused)
            }
        }
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        at: nazm_span::Span,
        expected: Type,
    ) -> Result<ConstValue, Refused> {
        use numeric::{Arith, Bit, Cmp};
        if matches!(op, BinOp::And | BinOp::Or) {
            let l = self.value(lhs, Type::Bool)?;
            let ConstValue::Bool(a) = l else {
                return Err(self.refuse(at, "combine values that are not `Bool`s with this"));
            };
            if (op == BinOp::And && !a) || (op == BinOp::Or && a) {
                return Ok(ConstValue::Bool(a));
            }
            return self.value(rhs, Type::Bool);
        }
        // A comparison's operands are typed by each other; arithmetic's by what is expected.
        let hint = if op.is_comparison() {
            Type::Int
        } else {
            expected
        };
        let shift = matches!(op, BinOp::Shl | BinOp::Shr);
        let l = self.value(lhs, hint)?;
        let r = self.value(rhs, if shift { Type::Int } else { l.ty() })?;
        match (&l, &r) {
            (ConstValue::Num(a), ConstValue::Num(b)) => {
                if !shift && a.kind != b.kind {
                    return Err(self.refuse(at, "combine two numeric types"));
                }
                let (a, b) = (*a, *b);
                let arith = |o: Arith| numeric::arith(o, a, b).map(ConstValue::Num);
                let bit = |o: Bit| numeric::bit(o, a, b).map(ConstValue::Num);
                let cmp = |o: Cmp| Ok(ConstValue::Bool(numeric::compare(o, a, b)));
                let out = match op {
                    BinOp::Add => arith(Arith::Add),
                    BinOp::Sub => arith(Arith::Sub),
                    BinOp::Mul => arith(Arith::Mul),
                    BinOp::Div => arith(Arith::Div),
                    BinOp::Rem => arith(Arith::Rem),
                    BinOp::BitAnd if a.kind.is_integer() => bit(Bit::And),
                    BinOp::BitOr if a.kind.is_integer() => bit(Bit::Or),
                    BinOp::BitXor if a.kind.is_integer() => bit(Bit::Xor),
                    BinOp::Shl if a.kind.is_integer() => bit(Bit::Shl),
                    BinOp::Shr if a.kind.is_integer() => bit(Bit::Shr),
                    BinOp::Eq => cmp(Cmp::Eq),
                    BinOp::NotEq => cmp(Cmp::NotEq),
                    BinOp::Lt => cmp(Cmp::Lt),
                    BinOp::LtEq => cmp(Cmp::LtEq),
                    BinOp::Gt => cmp(Cmp::Gt),
                    BinOp::GtEq => cmp(Cmp::GtEq),
                    _ => return Err(self.refuse(at, "apply this operator to these values")),
                };
                out.map_err(|err| self.fails(at, err))
            }
            (ConstValue::Bool(a), ConstValue::Bool(b)) => match op {
                BinOp::Eq => Ok(ConstValue::Bool(a == b)),
                BinOp::NotEq => Ok(ConstValue::Bool(a != b)),
                _ => Err(self.refuse(at, "apply this operator to `Bool`s")),
            },
            (ConstValue::Str(a), ConstValue::Str(b)) => match op {
                BinOp::Eq => Ok(ConstValue::Bool(a == b)),
                BinOp::NotEq => Ok(ConstValue::Bool(a != b)),
                _ => Err(self.refuse(at, "apply this operator to `Str`s")),
            },
            _ => Err(self.refuse(at, "combine values of two types")),
        }
    }
}

/// A stand-in value of `ty`, for a constant already refused: never lowered, since a program
/// with a refusal is not.
fn placeholder(ty: Type) -> ConstValue {
    match ty.num() {
        Some(k) => ConstValue::Num(NumVal { kind: k, bits: 0 }),
        None if ty == Type::Bool => ConstValue::Bool(false),
        None => ConstValue::Str(Vec::new()),
    }
}

/// A constant's declared type: a numeric type, `Bool` or `Str`.
fn declared(t: &nazm_syntax::TypeName) -> Option<Type> {
    if t.args.is_some() || t.function.is_some() || t.qualifier.is_some() {
        return None;
    }
    match Type::from_name(&t.name) {
        Some(ty @ (Type::Int | Type::Bool | Type::Str | Type::Num(_))) => Some(ty),
        _ => None,
    }
}

fn show(t: Type) -> String {
    t.builtin_name().unwrap_or("this type").to_owned()
}
