//! Arithmetic, comparison and equality on values, and the runtime failures they report.

use super::value::Value;
use super::*;
use nazm_sema::fnv;

/// A condition's value, once something has established it produced one.
///
/// Not a `Bool` here means checking passed something it should not have, so this
/// reports rather than panics: a checker bug should be readable.
pub(super) fn as_bool(v: &Value, cond: &cir::Expr) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(fail(
            codes::CONDITION_NOT_BOOL,
            cond.at,
            format!("a condition must be `Bool`, but this is `{other}`"),
        )),
    }
}

/// An array index outside the array (Gate 2): the sentence the compiled program writes too.
pub(super) fn index_outside(at: Span, i: i64, n: usize) -> RuntimeError {
    RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(
                codes::INDEX_OUT_OF_BOUNDS,
                at,
                "this index is outside the array",
            )
            .help(format!("asked for element {i} of an array of {n}")),
        ),
    }
}

pub(super) fn no_such_place(at: Span) -> RuntimeError {
    fail(
        codes::UNDEFINED_VARIABLE,
        at,
        "this is not a place that can be written to",
    )
}

/// Checked `Int` arithmetic, failing exactly where `docs/user/spec.md` says it does.
pub(super) fn arith(
    op: cir::ArithOp,
    l: &Value,
    r: &Value,
    at: Span,
) -> Result<Value, RuntimeError> {
    use cir::ArithOp::{Add, Div, Mul, Rem, Sub};
    use Value::Int;

    let overflow = |what: &str| RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(
                codes::INTEGER_OVERFLOW,
                at,
                format!("this {what} overflows `Int`"),
            )
            .help(
                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
            ),
        ),
    };
    match (op, l, r) {
        (Add, Int(a), Int(b)) => a
            .checked_add(*b)
            .map(Int)
            .ok_or_else(|| overflow("addition")),
        (Sub, Int(a), Int(b)) => a
            .checked_sub(*b)
            .map(Int)
            .ok_or_else(|| overflow("subtraction")),
        (Mul, Int(a), Int(b)) => a
            .checked_mul(*b)
            .map(Int)
            .ok_or_else(|| overflow("multiplication")),
        (Div | Rem, Int(_), Int(0)) => Err(RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::DIVISION_BY_ZERO, at, "division by zero")
                    .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`"),
            ),
        }),
        // `checked_div` also catches i64::MIN / -1, which overflows rather than dividing —
        // the one division that is not a zero check.
        (Div, Int(a), Int(b)) => a
            .checked_div(*b)
            .map(Int)
            .ok_or_else(|| overflow("division")),
        // `i64::MIN % -1` is mathematically 0 and 0 is representable, so nothing overflows.
        // `checked_rem` says `None` anyway, because the hardware instruction that computes
        // it faults on the *quotient*. Inheriting that would make the rule "`%` fails on a
        // zero divisor, and also on one exact pair of operands" — a wart an agent would have
        // to memorise. `wrapping_rem` is the correct answer here, and the zero divisor is
        // already handled above.
        (Rem, Int(a), Int(b)) => Ok(Int(a.wrapping_rem(*b))),
        (op, Value::Num(a), Value::Num(b)) => {
            let nop = match op {
                Add => numeric::Arith::Add,
                Sub => numeric::Arith::Sub,
                Mul => numeric::Arith::Mul,
                Div => numeric::Arith::Div,
                Rem => numeric::Arith::Rem,
            };
            let what = match op {
                Add => "addition",
                Sub => "subtraction",
                Mul => "multiplication",
                Div | Rem => "division",
            };
            numeric::arith(nop, *a, *b)
                .map(from_num)
                .map_err(|err| num_failure(err, at, what, a.kind))
        }
        // Unreachable after checking. A diagnostic rather than a panic.
        (op, a, b) => Err(fail(
            codes::INVALID_OPERANDS,
            at,
            format!(
                "`{}` does not apply to `{}` and `{}`",
                op.symbol(),
                type_name(a),
                type_name(b)
            ),
        )),
    }
}

/// A value as a number of the shared semantics, `Int` included (Gate 2).
pub(super) fn as_num(v: &Value) -> Option<NumVal> {
    match v {
        Value::Int(i) => NumVal::int(NumKind::I64, i128::from(*i)),
        Value::Num(n) => Some(*n),
        _ => None,
    }
}

/// A number as a value: an `Int` for that kind, a [`Value::Num`] for every other.
pub(super) fn from_num(n: NumVal) -> Value {
    if n.kind == NumKind::I64 {
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        Value::Int(n.bits as i64)
    } else {
        Value::Num(n)
    }
}

/// The failure a numeric operation stops the program with — the same codes and sentences the
/// compiled program reports.
pub(super) fn num_failure(err: NumError, at: Span, what: &str, kind: NumKind) -> RuntimeError {
    let d = match err {
        NumError::Overflow => Diagnostic::error(
            codes::INTEGER_OVERFLOW,
            at,
            format!("this {what} overflows `{}`", kind.name()),
        )
        .help("overflow stops the program rather than wrapping; `wrapping_add` and its kin wrap"),
        NumError::DivideByZero => {
            Diagnostic::error(codes::DIVISION_BY_ZERO, at, "division by zero")
                .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`")
        }
        NumError::ShiftRange => Diagnostic::error(
            codes::SHIFT_OUT_OF_RANGE,
            at,
            format!("this shift amount is outside `0..{}`", kind.bits()),
        )
        .help("a shift by a negative amount, or by the width or more, stops the program"),
    };
    RuntimeError {
        diagnostic: Box::new(d),
    }
}

/// A comparison. Ordering is a number's, or the derived order (L6) of anything else that has
/// one; equality is [`equal`], the language's law, and `!=` is its negation rather than a
/// second walk that could drift from it.
pub(super) fn compare(
    op: cir::CmpOp,
    l: &Value,
    r: &Value,
    at: Span,
    orders: &cir::Orders,
) -> Result<Value, RuntimeError> {
    use cir::CmpOp::{Eq, Gt, GtEq, Lt, LtEq, NotEq};
    use Value::{Bool, Int};

    let same = if matches!(op, Eq | NotEq) {
        equal(l, r)
    } else {
        None
    };
    let ordering = |o: numeric::Cmp, a: &NumVal, b: &NumVal| Ok(Bool(numeric::compare(o, *a, *b)));
    match (op, l, r) {
        (Lt, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::Lt, a, b),
        (LtEq, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::LtEq, a, b),
        (Gt, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::Gt, a, b),
        (GtEq, Value::Num(a), Value::Num(b)) => ordering(numeric::Cmp::GtEq, a, b),
        (Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
        (LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
        (Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
        (GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),
        (Lt | LtEq | Gt | GtEq, a, b) if order(a, b, orders).is_some() => {
            let o = order(a, b, orders).unwrap_or(std::cmp::Ordering::Equal);
            Ok(Bool(match op {
                Lt => o.is_lt(),
                LtEq => o.is_le(),
                Gt => o.is_gt(),
                _ => o.is_ge(),
            }))
        }
        (Eq, _, _) if same.is_some() => Ok(Bool(same == Some(true))),
        (NotEq, _, _) if same.is_some() => Ok(Bool(same == Some(false))),
        // Unreachable after checking. A diagnostic rather than a panic.
        (op, a, b) => Err(fail(
            codes::INVALID_OPERANDS,
            at,
            format!(
                "`{}` does not apply to `{}` and `{}`",
                op.symbol(),
                type_name(a),
                type_name(b)
            ),
        )),
    }
}

/// The derived order (L6, `docs/user/spec.md`, *Ordering and hashing are derived*): integers by
/// value, `false` before `true`, a `Str` by its bytes, a record by its fields in the canonical
/// order — by name, which for a tuple is its positions — and an enum by its variants in the
/// canonical order, then that variant's payload likewise. `None` for what has none, which
/// checking refuses before a program runs.
///
/// Recursive, and bounded: a type nests at most [`nazm_sema::MAX_TYPE_DEPTH`] deep, and nothing
/// reached through an indirection orders.
fn order(a: &Value, b: &Value, orders: &cir::Orders) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let fields = |canon: &[u32], xs: &[Value], ys: &[Value]| -> Option<Ordering> {
        for i in canon {
            let o = order(xs.get(*i as usize)?, ys.get(*i as usize)?, orders)?;
            if o != Ordering::Equal {
                return Some(o);
            }
        }
        Some(Ordering::Equal)
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Num(x), Value::Num(y)) if x.kind.is_integer() => {
            if numeric::compare(numeric::Cmp::Lt, *x, *y) {
                Some(Ordering::Less)
            } else if numeric::compare(numeric::Cmp::Eq, *x, *y) {
                Some(Ordering::Equal)
            } else {
                Some(Ordering::Greater)
            }
        }
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.bytes().cmp(y.bytes())),
        (Value::Record(x), Value::Record(y)) => {
            let canon = orders.records.get(x.def().index())?;
            fields(canon, &x.fields, &y.fields)
        }
        (Value::Enum(x), Value::Enum(y)) => {
            let e = orders.enums.get(x.def().index())?;
            let rank = |v: u32| e.variants.iter().position(|d| *d == v);
            let (rx, ry) = (rank(x.variant)?, rank(y.variant)?);
            if rx != ry {
                return Some(rx.cmp(&ry));
            }
            fields(e.payloads.get(x.variant as usize)?, &x.fields, &y.fields)
        }
        _ => None,
    }
}

/// The derived hash (L6, `docs/user/spec.md`, *Ordering and hashing are derived*): an integer's
/// 64-bit pattern, a `Bool` as 0 or 1, a `Str`'s bytes — the library's impls — and a record or
/// tuple from `hash_u64(0)`, an enum value from its variant's name, folding each component in
/// the canonical order with `hash_combine`. `None` for what has none, which checking refuses.
pub(super) fn derived_hash(v: &Value, orders: &cir::Orders) -> Option<u64> {
    let fold = |mut h: u64, canon: &[u32], xs: &[Value]| -> Option<u64> {
        for i in canon {
            h = fnv::combine(h, derived_hash(xs.get(*i as usize)?, orders)?);
        }
        Some(h)
    };
    match v {
        #[allow(clippy::cast_sign_loss)]
        Value::Int(x) => Some(fnv::word(*x as u64)),
        // A 128-bit integer (R1b, L13b): its low word's hash and its high word's, combined.
        #[allow(clippy::cast_possible_truncation)]
        Value::Num(n) if n.kind.bits() == 128 => Some(fnv::combine(
            fnv::word(n.bits as u64),
            fnv::word((n.bits >> 64) as u64),
        )),
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Value::Num(n) if n.kind.is_integer() => Some(fnv::word(n.as_i128() as u64)),
        Value::Bool(b) => Some(fnv::word(u64::from(*b))),
        Value::Str(s) => Some(fnv::bytes(s.bytes())),
        Value::Record(r) => fold(
            fnv::word(0),
            orders.records.get(r.def().index())?,
            &r.fields,
        ),
        Value::Enum(e) => {
            let o = orders.enums.get(e.def().index())?;
            let name = o.names.get(e.variant as usize)?;
            fold(
                fnv::bytes(name.as_bytes()),
                o.payloads.get(e.variant as usize)?,
                &e.fields,
            )
        }
        _ => None,
    }
}

/// Built-in equality, by the language's law rather than the host's. N13.
///
/// `Int`, `Bool` and `Str` compare as they always have — a `Str` by its bytes, never by
/// whether two views share a backing. A record compares every field, and an enum its
/// active variant and then **that variant's payload alone**: a value here carries no
/// storage for an inactive variant, so there is nothing else it could read. No definition
/// id is compared, because checking has already established that both sides have one type.
///
/// `None` for a value the language gives no equality — a sequence, a `Vec`, a `Chan` —
/// which checking refuses (`N0304`) before a program runs. [`Value`]'s own `PartialEq`
/// answers those by identity so the rest of `Value` can be compared, and that answer is
/// the host's convenience, not the language's: it is not what `==` means, and this is.
///
/// Recursive, and bounded: a type nests at most [`nazm_sema::MAX_TYPE_DEPTH`] deep.
fn equal(a: &Value, b: &Value) -> Option<bool> {
    let all = |xs: &[Value], ys: &[Value]| -> Option<bool> {
        if xs.len() != ys.len() {
            return None;
        }
        for (x, y) in xs.iter().zip(ys) {
            if !equal(x, y)? {
                return Some(false);
            }
        }
        Some(true)
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x == y),
        // IEEE 754's for a float (Gate 2): a NaN equals nothing.
        (Value::Num(x), Value::Num(y)) => Some(numeric::compare(numeric::Cmp::Eq, *x, *y)),
        // Element by element (Gate 2).
        (Value::Array(x), Value::Array(y)) => all(x, y),
        (Value::Bool(x), Value::Bool(y)) => Some(x == y),
        (Value::Str(x), Value::Str(y)) => Some(x.bytes() == y.bytes()),
        (Value::Record(x), Value::Record(y)) => all(&x.fields, &y.fields),
        (Value::Enum(x), Value::Enum(y)) => {
            if x.variant != y.variant {
                return Some(false);
            }
            all(&x.fields, &y.fields)
        }
        // A `Box` or `Chain` (L6), which the interpreter holds as a `Vec`: by what it holds.
        // Checking refuses `==` on a `Vec` itself, so only those two reach here. One storage
        // is equal to itself, and is not locked twice.
        (Value::Vec(x), Value::Vec(y)) => {
            if std::sync::Arc::ptr_eq(x, y) {
                return Some(true);
            }
            let (xs, ys) = (x.lock().ok()?.clone(), y.lock().ok()?.clone());
            if xs.len() != ys.len() {
                return Some(false);
            }
            all(&xs, &ys)
        }
        _ => None,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Num(n) => n.kind.name(),
        Value::Array(_) => "an array",
        Value::Bool(_) => "Bool",
        Value::Str(_) => "Str",
        Value::Chan(_) => "Chan",
        Value::Bytes(_) => "Bytes",
        Value::OsHandle(_) => "OsHandle",
        Value::Atomic(_) => "Atomic",
        Value::Select(_) => "Select",
        // Only reached by a built-in handed the wrong type, which checking has already
        // refused — and no built-in takes a record, so no message can name one usefully.
        Value::Record(_) => "a record",
        Value::Enum(_) => "an enum",
        Value::Vec(_) => "a `Vec`",
        Value::Cap(k) => k.type_name(),
        Value::Func(_) => "a function value",
    }
}
