//! Loop bounds (N63, `docs/architecture.md` §7.65): what the compiler establishes about how many
//! times a `while` turns, read off Core IR.
//!
//! One form is bounded: a counter set to a literal as its last write before the loop, compared
//! with a literal (or an immutable local bound to one), and stepped once per turn by a positive
//! literal at the top level of the body, with no `continue` to the loop. Every other loop is
//! `None`, with the reason — the compiler does not know, which is not the same as unbounded.

use nazm_core::cir::ir::{
    ArithOp, Block, CmpOp, Const, Expr, ExprKind, Function, LocalId, LocalOrigin, LoopId, Program,
    Stmt,
};
use nazm_span::Span;

/// One loop's bound, per entry to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopBound {
    pub function: String,
    pub at: Span,
    /// The most turns one entry to the loop makes, or `None` where the compiler does not know.
    pub bound: Option<u128>,
    /// Why it is `None`; empty for a bound.
    pub reason: &'static str,
}

/// Every loop of the program's functions, in function and source order.
#[must_use]
pub fn loops(program: &Program) -> Vec<LoopBound> {
    let mut out = Vec::new();
    for f in program.functions.iter().filter(|f| f.foreign.is_none()) {
        in_block(f, &f.body, &mut out);
    }
    out
}

fn in_block(f: &Function, b: &Block, out: &mut Vec<LoopBound>) {
    for (k, s) in b.stmts.iter().enumerate() {
        if let Stmt::Loop { id, cond, body, at } = s {
            let before = b.stmts.get(..k).unwrap_or(&[]);
            let (bound, reason) = match bound_of(f, before, *id, cond, body) {
                Ok(n) => (Some(n), ""),
                Err(r) => (None, r),
            };
            out.push(LoopBound {
                function: f.name.clone(),
                at: *at,
                bound,
                reason,
            });
        }
        stmt_blocks(s, &mut |inner| in_block(f, inner, out));
    }
    if let Some(t) = &b.tail {
        expr_blocks(t, &mut |inner| in_block(f, inner, out));
    }
}

/// The trip bound of one loop, or why there is none.
#[allow(
    clippy::integer_division,
    reason = "a trip count is a whole number of turns, rounded as the arithmetic says"
)]
fn bound_of(
    f: &Function,
    before: &[Stmt],
    id: LoopId,
    cond: &Expr,
    body: &Block,
) -> Result<u128, &'static str> {
    let ExprKind::Compare { op, lhs, rhs, .. } = &cond.kind else {
        return Err("its condition is not a comparison of a counter with a literal");
    };
    let ExprKind::Local(i) = lhs.kind else {
        return Err("its condition is not a comparison of a counter with a literal");
    };
    let limit =
        literal(f, rhs).ok_or("its condition does not compare the counter with a literal")?;
    let ascending = match op {
        CmpOp::Lt | CmpOp::LtEq => true,
        CmpOp::Gt | CmpOp::GtEq => false,
        CmpOp::Eq | CmpOp::NotEq => return Err("its condition is an equality, not an ordering"),
    };
    // The start: the last write to the counter before the loop, in this block.
    let mut start = None;
    for s in before.iter().rev() {
        match s {
            Stmt::Let { local, value } | Stmt::Assign { local, value, .. } if *local == i => {
                if let Stmt::Assign { path, .. } = s {
                    if !path.is_empty() {
                        return Err("the counter's start is not a literal");
                    }
                }
                start = Some(literal(f, value).ok_or("the counter's start is not a literal")?);
                break;
            }
            s if writes(s, i) > 0 => {
                return Err(
                    "the counter is written before the loop where the compiler does not follow",
                );
            }
            _ => {}
        }
    }
    let start = start.ok_or("the counter's start is not set in the loop's block")?;
    // The step: the body's one write to the counter, at its top level.
    let total: usize = body.stmts.iter().map(|s| writes(s, i)).sum::<usize>()
        + body.tail.as_ref().map_or(0, |t| expr_writes(t, i));
    if total != 1 {
        return Err("the counter is not written exactly once in the body");
    }
    let step = body
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Assign {
                local, path, value, ..
            } if *local == i && path.is_empty() => Some(value),
            _ => None,
        })
        .ok_or("the counter's one write is not at the top level of the body")?;
    let ExprKind::Arith {
        op: aop,
        lhs: sl,
        rhs: sr,
    } = &step.kind
    else {
        return Err("the counter is not stepped by a literal");
    };
    if !matches!(sl.kind, ExprKind::Local(j) if j == i) {
        return Err("the counter is not stepped by a literal");
    }
    let c = literal(f, sr).ok_or("the counter is not stepped by a literal")?;
    let steps_right = match aop {
        ArithOp::Add => ascending,
        ArithOp::Sub => !ascending,
        _ => false,
    };
    if c <= 0 || !steps_right {
        return Err("the counter does not step towards its limit");
    }
    if continues(body, id) {
        return Err("a `continue` to the loop skips the counter's step");
    }
    let (a, b, c) = (i128::from(start), i128::from(limit), i128::from(c));
    let span = if ascending { b - a } else { a - b };
    let inclusive = matches!(op, CmpOp::LtEq | CmpOp::GtEq);
    let turns = if span < 0 || (span == 0 && !inclusive) {
        0
    } else if inclusive {
        span / c + 1
    } else {
        (span + c - 1) / c
    };
    Ok(u128::try_from(turns).unwrap_or(0))
}

/// The `Int` an expression is, where it is a literal, a negated one, or an immutable local
/// bound to one.
fn literal(f: &Function, e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Neg(x) => literal(f, x).and_then(i64::checked_neg),
        ExprKind::Const(k) => match f.constant(*k)? {
            Const::Int(n) => Some(*n),
            _ => None,
        },
        ExprKind::Local(l) => {
            let local = f.local(*l)?;
            if !matches!(local.origin, LocalOrigin::Binding { mutable: false }) {
                return None;
            }
            find_let(&f.body, *l).and_then(|v| match &v.kind {
                ExprKind::Const(k) => match f.constant(*k)? {
                    Const::Int(n) => Some(*n),
                    _ => None,
                },
                _ => None,
            })
        }
        _ => None,
    }
}

/// The value an immutable local is bound to: a local is bound exactly once.
fn find_let(b: &Block, l: LocalId) -> Option<&Expr> {
    for s in &b.stmts {
        if let Stmt::Let { local, value } = s {
            if *local == l {
                return Some(value);
            }
        }
        let mut found = None;
        stmt_blocks(s, &mut |inner| {
            if found.is_none() {
                found = find_let(inner, l);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    let mut found = None;
    if let Some(t) = &b.tail {
        expr_blocks(t, &mut |inner| {
            if found.is_none() {
                found = find_let(inner, l);
            }
        });
    }
    found
}

/// How many times `s` writes `l`, anywhere inside it.
fn writes(s: &Stmt, l: LocalId) -> usize {
    let own = match s {
        Stmt::Let { local, .. } | Stmt::Assign { local, .. } if *local == l => 1,
        _ => 0,
    };
    let mut nested = 0;
    stmt_blocks(s, &mut |b| {
        nested += b.stmts.iter().map(|t| writes(t, l)).sum::<usize>()
            + b.tail.as_ref().map_or(0, |t| expr_writes(t, l));
    });
    own + nested
}

fn expr_writes(e: &Expr, l: LocalId) -> usize {
    let mut n = 0;
    expr_blocks(e, &mut |b| {
        n += b.stmts.iter().map(|t| writes(t, l)).sum::<usize>()
            + b.tail.as_ref().map_or(0, |t| expr_writes(t, l));
    });
    n
}

/// Whether a `continue` to `id` is anywhere in `b`.
fn continues(b: &Block, id: LoopId) -> bool {
    b.stmts.iter().any(|s| {
        matches!(s, Stmt::Continue { target, .. } if *target == id) || {
            let mut found = false;
            stmt_blocks(s, &mut |inner| found = found || continues(inner, id));
            found
        }
    }) || b.tail.as_ref().is_some_and(|t| {
        let mut found = false;
        expr_blocks(t, &mut |inner| found = found || continues(inner, id));
        found
    })
}

/// Every region directly inside a statement: its own, and those in its expressions.
fn stmt_blocks<'a>(s: &'a Stmt, visit: &mut dyn FnMut(&'a Block)) {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Eval(value) => {
            expr_blocks(value, visit);
        }
        Stmt::Return { value, .. } => expr_blocks(value, visit),
        Stmt::If {
            cond,
            then,
            otherwise,
            ..
        } => {
            expr_blocks(cond, visit);
            visit(then);
            if let Some(o) = otherwise {
                visit(o);
            }
        }
        Stmt::Match(m) => {
            expr_blocks(&m.scrutinee, visit);
            for a in &m.arms {
                visit(&a.body);
            }
        }
        Stmt::Loop { cond, body, .. } => {
            expr_blocks(cond, visit);
            visit(body);
        }
        Stmt::Block(b) | Stmt::Scope { body: b, .. } => visit(b),
        Stmt::Spawn { args, .. } => {
            for a in args {
                expr_blocks(a, visit);
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

/// Every region directly inside an expression, at any depth of expressions.
fn expr_blocks<'a>(e: &'a Expr, visit: &mut dyn FnMut(&'a Block)) {
    match &e.kind {
        ExprKind::Const(_)
        | ExprKind::Local(_)
        | ExprKind::Closure { .. }
        | ExprKind::FnValue(_) => {}
        ExprKind::Neg(x)
        | ExprKind::Complement(x)
        | ExprKind::Convert { operand: x, .. }
        | ExprKind::Fits { operand: x, .. }
        | ExprKind::ArrayRepeat { value: x, .. }
        | ExprKind::Field { base: x, .. } => expr_blocks(x, visit),
        ExprKind::Array { elems } => {
            for x in elems {
                expr_blocks(x, visit);
            }
        }
        ExprKind::Index { base, index } => {
            expr_blocks(base, visit);
            expr_blocks(index, visit);
        }
        ExprKind::Arith { lhs, rhs, .. }
        | ExprKind::Bits { lhs, rhs, .. }
        | ExprKind::Compare { lhs, rhs, .. } => {
            expr_blocks(lhs, visit);
            expr_blocks(rhs, visit);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for a in args {
                expr_blocks(a, visit);
            }
        }
        ExprKind::CallIndirect { callee, args, .. } => {
            expr_blocks(callee, visit);
            for a in args {
                expr_blocks(a, visit);
            }
        }
        ExprKind::CallTrait { args, .. } => {
            for a in args {
                expr_blocks(a, visit);
            }
        }
        ExprKind::Record { fields, .. } | ExprKind::Variant { fields, .. } => {
            for i in fields {
                expr_blocks(&i.value, visit);
            }
        }
        ExprKind::Match(m) => {
            expr_blocks(&m.scrutinee, visit);
            for a in &m.arms {
                visit(&a.body);
            }
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            expr_blocks(cond, visit);
            visit(then);
            visit(otherwise);
        }
        ExprKind::Block(b) => visit(b),
    }
}
