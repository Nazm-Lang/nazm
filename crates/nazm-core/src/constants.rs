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

/// Compute every constant of a module, refusing what cannot be computed. `imported` are the
/// constants its imports export (L8), by the name this module writes, which its own may use.
pub fn evaluate(
    decls: &[&Const],
    imported: Vec<(String, (Type, ConstValue))>,
    errors: &mut Vec<Diagnostic>,
) -> Constants {
    let by_name: HashMap<&str, &Const> = decls.iter().map(|c| (c.name.as_str(), *c)).collect();
    let mut decl_spans: HashMap<String, nazm_span::Span> = HashMap::new();
    for c in decls {
        decl_spans.entry(c.name.clone()).or_insert(c.name_span);
    }
    let mut ev = Evaluator {
        decls: by_name,
        // An import's constants first: a module's own of the same name hides one.
        done: imported.into_iter().collect(),
        cells: HashMap::new(),
        active: Vec::new(),
        refs: Vec::new(),
        errors,
        types: None,
        aggregates: HashMap::new(),
    };
    for c in decls {
        ev.done.remove(&c.name);
    }
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
        // A constant is exported by its value (L8); a static is storage, its module's.
        if let Some(p) = c.pub_span.filter(|_| c.is_static) {
            ev.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    p,
                    format!(
                        "`{}` cannot be exported: a static is its own module's",
                        c.name
                    ),
                )
                .help(
                    "a static is storage with one address in its module; export a function \
                     that reads it, or a constant if its value never changes",
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

/// The interrupt a function's `@interrupt` names (Gate 3), held to the attribute's rule elsewhere.
#[must_use]
pub fn interrupt_of(attrs: &[nazm_syntax::ast::Attribute]) -> Option<String> {
    string_attr(attrs, "interrupt")
}

/// A function's `@task(priority = "N", period = "P")` (Gate 3): its priority and period, where both
/// are written and in range — held to the attribute's rule elsewhere.
#[must_use]
pub fn task_of(attrs: &[nazm_syntax::ast::Attribute]) -> Option<(u32, u64)> {
    let a = attrs.iter().find(|a| a.name == "task")?;
    let mut priority = None;
    let mut period = None;
    for arg in a.args.as_deref()? {
        if let nazm_syntax::ast::AttrArg::Pair { key, value, .. } = arg {
            match key.as_str() {
                "priority" => priority = value.parse::<u32>().ok().filter(|p| *p <= 255),
                "period" => period = value.parse::<u64>().ok().filter(|p| *p >= 1),
                _ => return None,
            }
        }
    }
    Some((priority?, period?))
}

/// The name a declaration's `@section` gives (Gate 3), held to the attribute's rule elsewhere.
#[must_use]
pub fn section_of(attrs: &[nazm_syntax::ast::Attribute]) -> Option<String> {
    string_attr(attrs, "section")
}

/// The one string argument of the attribute `name`, where it is written with one.
fn string_attr(attrs: &[nazm_syntax::ast::Attribute], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|a| a.name == name)
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
    /// The module's types, in the second phase (L8): `None` while only scalars are computed,
    /// before any record is known.
    types: Option<&'a nazm_sema::UserTypes>,
    /// Each constant of a record, enum, tuple or array type, by name: its declared type,
    /// resolved (L8).
    aggregates: HashMap<String, Type>,
}

/// The second phase (R1a, L8, `docs/spec.md`, *Constant tables and exported constants*): each
/// constant of a record, enum, tuple or array type — `tys`, its declared type resolved where the
/// module's types are known — computed into `consts` beside the scalars the first phase did.
pub fn evaluate_aggregates(
    consts: &mut Constants,
    decls: &[&Const],
    tys: &[(String, Type)],
    types: &nazm_sema::UserTypes,
    errors: &mut Vec<Diagnostic>,
) {
    let mut ev = Evaluator {
        decls: decls.iter().map(|c| (c.name.as_str(), *c)).collect(),
        done: consts.values.clone(),
        cells: HashMap::new(),
        active: Vec::new(),
        refs: Vec::new(),
        errors,
        types: Some(types),
        aggregates: tys.iter().cloned().collect(),
    };
    for (name, _) in tys {
        ev.constant(name);
    }
    consts.values = ev.done;
    consts.refs.extend(ev.refs);
    consts.refs.sort_by_key(|(s, _)| (s.file, s.start));
    consts.refs.dedup();
}

/// Whether a constant's declared type is computed in the second phase (L8): anything but a
/// number, a `Bool` or a `Str` — a record, an enum, a tuple or an array, or a mistake the second
/// phase reports.
#[must_use]
pub fn is_aggregate(c: &Const) -> bool {
    !c.is_static && declared(&c.ty).is_none()
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
                        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
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
        // A record, enum, tuple or array (L8): the second phase's, where types are known.
        if !decl.is_static && declared(&decl.ty).is_none() {
            if let Some(ty) = self.aggregates.get(name).copied() {
                return self.aggregate(name, decl, ty);
            }
            if !self.active.is_empty() {
                self.errors.push(Diagnostic::error(
                    codes::NOT_A_CONSTANT,
                    decl.name_span,
                    format!("`{name}` is not a number, a `Bool` or a `Str`, which this needs"),
                ));
            }
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

    /// A constant of a record, enum, tuple or array type (L8).
    fn aggregate(&mut self, name: &str, decl: &Const, ty: Type) -> Option<(Type, ConstValue)> {
        let types = self.types?;
        if let Some(handle) = types.not_value(ty, true) {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_A_CONSTANT,
                    decl.ty.span,
                    format!(
                        "a constant is a value, and `{}` holds a `{}`",
                        types.show(ty),
                        types.show(handle)
                    ),
                )
                .help(
                    "a constant is a number, a `Bool`, a `Str`, or a record, enum, tuple or \
                     array of them — computed when the program is checked, with no storage",
                ),
            );
            return None;
        }
        self.active.push(name.to_owned());
        let value = self.agg(&decl.value, ty).ok();
        self.active.pop();
        // Still declared at its type when refused, so a use is not a second error.
        let value = value.unwrap_or_else(|| ConstValue::Array {
            ty,
            elems: Vec::new(),
        });
        self.done.insert(name.to_owned(), (ty, value.clone()));
        Some((ty, value))
    }

    /// `e` as a value of `ty`, a record, enum, tuple or array built of constant parts, or a scalar
    /// constant expression (L8).
    fn agg(&mut self, e: &Expr, ty: Type) -> Result<ConstValue, Refused> {
        let Some(types) = self.types else {
            return Err(Refused);
        };
        if matches!(ty, Type::Int | Type::Bool | Type::Str | Type::Num(_)) {
            let v = self.value(e, ty)?;
            if v.ty() != ty {
                return Err(self.mismatch(e.span(), ty));
            }
            return Ok(v);
        }
        if let Expr::Var { name, span } = e {
            let own = self.decls.contains_key(name.as_str());
            // Its own constant, or one an import exports (L8).
            if !own && !self.done.contains_key(name.as_str()) {
                return Err(self.refuse(*span, &format!("name `{name}`, which is not a constant")));
            }
            if own {
                self.refs.push((*span, name.clone()));
            }
            return match self.constant(name) {
                Some((t, v)) if t == ty => Ok(v),
                Some(_) => Err(self.mismatch(*span, ty)),
                None => Err(Refused),
            };
        }
        let base = |n: &str| types.show(ty).split('[').next() == Some(n);
        match (ty, e) {
            (Type::Record(_), Expr::Tuple { elems, span }) => {
                let Some(want) = types.tuple_elements(ty) else {
                    return Err(self.mismatch(*span, ty));
                };
                if want.len() != elems.len() {
                    return Err(self.mismatch(*span, ty));
                }
                let fields = elems
                    .iter()
                    .zip(want)
                    .map(|(x, t)| self.agg(x, t))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ConstValue::Record { ty, fields })
            }
            (
                Type::Record(id),
                Expr::Construct {
                    ty: written,
                    fields: inits,
                    span,
                    ..
                },
            ) if base(written) && types.tuple_elements(ty).is_none() => {
                let Some(def) = types.records().get(id) else {
                    return Err(Refused);
                };
                let want: Vec<(String, Type)> =
                    def.fields.iter().map(|f| (f.name.clone(), f.ty)).collect();
                let fields = self.fields(&want, inits, *span)?;
                Ok(ConstValue::Record { ty, fields })
            }
            (
                Type::Enum(id),
                Expr::Variant {
                    ty: written,
                    variant,
                    variant_span,
                    fields: inits,
                    span,
                    ..
                },
            ) if base(written) => {
                let Some(def) = types.enums().get(id) else {
                    return Err(Refused);
                };
                let Some((vi, v)) = def
                    .variants
                    .iter()
                    .enumerate()
                    .find(|(_, v)| v.name == *variant)
                else {
                    return Err(self.mismatch(*variant_span, ty));
                };
                let want: Vec<(String, Type)> =
                    v.fields.iter().map(|f| (f.name.clone(), f.ty)).collect();
                let fields = self.fields(&want, inits, *span)?;
                Ok(ConstValue::Variant {
                    ty,
                    variant: u32::try_from(vi).unwrap_or(u32::MAX),
                    fields,
                })
            }
            (Type::Array(a), Expr::Array { elems, span }) => {
                let Some((elem, len)) = types.array_info(a) else {
                    return Err(Refused);
                };
                if elems.len() != len as usize {
                    return Err(self.mismatch(*span, ty));
                }
                let elems = elems
                    .iter()
                    .map(|x| self.agg(x, elem))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ConstValue::Array { ty, elems })
            }
            (Type::Array(a), Expr::ArrayRepeat { value, count, span }) => {
                let Some((elem, len)) = types.array_info(a) else {
                    return Err(Refused);
                };
                let n = self.value(count, Type::Int)?.as_int();
                if n != Some(i128::from(len)) {
                    return Err(self.mismatch(*span, ty));
                }
                let one = self.agg(value, elem)?;
                Ok(ConstValue::Array {
                    ty,
                    elems: vec![one; len as usize],
                })
            }
            (_, Expr::Call { span, .. }) => Err(self.refuse(*span, "call a function")),
            (_, other) => Err(self.mismatch(other.span(), ty)),
        }
    }

    /// A construction's fields as `want` declares them, each once, in declaration order (L8).
    fn fields(
        &mut self,
        want: &[(String, Type)],
        inits: &[nazm_syntax::FieldInit],
        at: nazm_span::Span,
    ) -> Result<Vec<ConstValue>, Refused> {
        if let Some(extra) = inits
            .iter()
            .find(|i| !want.iter().any(|(n, _)| *n == i.name))
        {
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                extra.name_span,
                format!("there is no field `{}` here", extra.name),
            ));
            return Err(Refused);
        }
        let mut out = Vec::with_capacity(want.len());
        for (name, t) in want {
            let Some(init) = inits.iter().find(|i| i.name == *name) else {
                self.errors.push(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    at,
                    format!("this constant's construction does not give `{name}`"),
                ));
                return Err(Refused);
            };
            out.push(self.agg(&init.value, *t)?);
        }
        Ok(out)
    }

    fn mismatch(&mut self, at: nazm_span::Span, ty: Type) -> Refused {
        let shown = self.types.map_or_else(|| show(ty), |t| t.show(ty));
        self.errors.push(Diagnostic::error(
            codes::TYPE_MISMATCH,
            at,
            format!("a constant of type `{shown}` is not this"),
        ));
        Refused
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
            Expr::Int { value, span } => self.integer((*value).into(), want, *span),
            Expr::WideInt { value, span } => self.integer((*value).into(), want, *span),
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
                        return self.integer((-i128::from(*value)).into(), want, *lit);
                    }
                }
                if let Expr::WideInt { value, span: lit } = operand.as_ref() {
                    if want.is_some_and(|k| k != NumKind::I64 && k.is_integer()) {
                        let v = nazm_sema::numeric::Wide::from_u128(*value).negate();
                        return self.integer(v, want, *lit);
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
        v: nazm_sema::numeric::Wide,
        want: Option<NumKind>,
        at: nazm_span::Span,
    ) -> Result<ConstValue, Refused> {
        let k = want.filter(|k| k.is_integer()).unwrap_or(NumKind::I64);
        match NumVal::int(k, v) {
            Some(n) => Ok(ConstValue::Num(n)),
            None => {
                let (lo, hi) = k.int_range().unwrap_or((
                    nazm_sema::numeric::Wide::ZERO,
                    nazm_sema::numeric::Wide::ZERO,
                ));
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
