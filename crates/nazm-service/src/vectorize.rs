//! Vectorisation (N66, `docs/internal/architecture.md` §7.68): one loop idiom, recognised in Core IR and
//! replaced by `ints_sum_from`, whose runtime is vectorised and exact; every other loop is left as it
//! is, with the reason.
//!
//! The idiom: a counter from `0`, compared with `ints_len(v)`, whose body is exactly
//! `s = s + ints_get(v, i); i = i + 1;`. The replacement is `s = ints_sum_from(s, v); i = ints_len(v);`
//! with the call at the addition's span, so an overflow is reported where the loop's addition
//! would have reported it: `ints_sum_from` fails exactly when one of the loop's running sums would.
//! The bounds checks the loop made are unnecessary — `i` never reaches `ints_len(v)` — and the
//! only effect of the loop's turns is on `s` and `i`, which the replacement sets.
//!
//! Native builds only: the interpreter keeps the scalar loop, so its iteration budget still
//! counts every turn.

use nazm_core::cir::ir::{ArithOp, Block, CmpOp, Const, Expr, ExprKind, LocalId, Program, Stmt};
use nazm_core::sema::{Intrinsic, Type};
use nazm_span::Span;

/// What happened to one loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopFact {
    pub function: String,
    pub at: Span,
    /// The idiom it was replaced by, or `None`.
    pub vectorised: Option<&'static str>,
    /// Why it was not, where it was not.
    pub reason: &'static str,
}

/// Rewrite every summation idiom of `program` in place, and say what became of every loop.
pub fn rewrite(program: &mut Program) -> Vec<LoopFact> {
    let mut facts = Vec::new();
    for f in &mut program.functions {
        if f.foreign.is_some() {
            continue;
        }
        let name = f.name.clone();
        let consts = f.consts.clone();
        in_block(&name, &consts, &mut f.body, &mut facts);
    }
    facts
}

/// What `rewrite` would do, without doing it.
#[must_use]
pub fn explain(program: &Program) -> Vec<LoopFact> {
    let mut copy = program.clone();
    rewrite(&mut copy)
}

fn in_block(name: &str, consts: &[Const], b: &mut Block, facts: &mut Vec<LoopFact>) {
    for k in 0..b.stmts.len() {
        let replacement = match b.stmts.get(k) {
            Some(Stmt::Loop { cond, body, at, .. }) => {
                let before = b.stmts.get(..k).unwrap_or(&[]);
                match idiom(consts, before, cond, body) {
                    Ok(r) => {
                        facts.push(LoopFact {
                            function: name.to_owned(),
                            at: *at,
                            vectorised: Some("summation"),
                            reason: "",
                        });
                        Some(r)
                    }
                    Err(why) => {
                        facts.push(LoopFact {
                            function: name.to_owned(),
                            at: *at,
                            vectorised: None,
                            reason: why,
                        });
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some(stmts) = replacement {
            if let Some(slot) = b.stmts.get_mut(k) {
                let at = match slot {
                    Stmt::Loop { at, .. } => *at,
                    _ => b.at,
                };
                *slot = Stmt::Block(Block {
                    stmts,
                    tail: None,
                    ends: nazm_core::cir::ir::Ends::FallsThrough,
                    owns: Vec::new(),
                    at,
                });
            }
            continue;
        }
        if let Some(s) = b.stmts.get_mut(k) {
            nested(name, consts, s, facts);
        }
    }
}

fn nested(name: &str, consts: &[Const], s: &mut Stmt, facts: &mut Vec<LoopFact>) {
    match s {
        Stmt::If {
            then, otherwise, ..
        } => {
            in_block(name, consts, then, facts);
            if let Some(o) = otherwise {
                in_block(name, consts, o, facts);
            }
        }
        Stmt::Match(m) => {
            for a in &mut m.arms {
                in_block(name, consts, &mut a.body, facts);
            }
        }
        Stmt::Loop { body, .. } | Stmt::Block(body) | Stmt::Scope { body, .. } => {
            in_block(name, consts, body, facts);
        }
        _ => {}
    }
}

fn local(e: &Expr) -> Option<LocalId> {
    match e.kind {
        ExprKind::Local(l) => Some(l),
        _ => None,
    }
}

fn int(consts: &[Const], e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Const(k) => match consts.get(k.index())? {
            Const::Int(n) => Some(*n),
            _ => None,
        },
        _ => None,
    }
}

/// The replacement statements for one loop, or why it is not the idiom.
fn idiom(
    consts: &[Const],
    before: &[Stmt],
    cond: &Expr,
    body: &Block,
) -> Result<Vec<Stmt>, &'static str> {
    const NOT: &str = "its condition is not `i < ints_len(v)` over a counter";
    let ExprKind::Compare {
        op: CmpOp::Lt,
        lhs,
        rhs,
        ..
    } = &cond.kind
    else {
        return Err(NOT);
    };
    let i = local(lhs).ok_or(NOT)?;
    let ExprKind::Builtin {
        op: Intrinsic::IntsLen,
        args,
        ..
    } = &rhs.kind
    else {
        return Err(NOT);
    };
    let v = args.first().and_then(local).ok_or(NOT)?;
    // The counter starts at 0: its last write before the loop, in this block — and nothing
    // between that write and the loop can write it again. Only a plain binding or assignment of
    // another local to a constant or a local may stand between them: an `if`, a loop, a block or
    // a call's argument could hold a write of the counter at any depth, and until Q1 one in a
    // nested `if` was missed, so the loop was summed from 0 instead of from where the program
    // left the counter (Q1-B-01).
    let at = before.iter().rposition(
        |s| matches!(s, Stmt::Let { local, .. } | Stmt::Assign { local, .. } if *local == i),
    );
    let start = at.and_then(|k| match before.get(k)? {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => int(consts, value),
        _ => None,
    });
    if start != Some(0) {
        return Err("its counter does not start at 0 in the loop's block");
    }
    let plain = |s: &Stmt| match s {
        Stmt::Let { local, value } | Stmt::Assign { local, value, .. } => {
            *local != i && matches!(value.kind, ExprKind::Const(_) | ExprKind::Local(_))
        }
        _ => false,
    };
    if !before
        .get(at.map_or(0, |k| k + 1)..)
        .unwrap_or_default()
        .iter()
        .all(plain)
    {
        return Err("something between its counter's `= 0` and the loop may write the counter");
    }
    let [Stmt::Assign {
        local: s,
        path: p1,
        value: add,
        ..
    }, Stmt::Assign {
        local: i2,
        path: p2,
        value: step,
        ..
    }] = body.stmts.as_slice()
    else {
        return Err("its body is not exactly `s = s + ints_get(v, i); i = i + 1;`");
    };
    if !p1.is_empty() || !p2.is_empty() || *i2 != i || *s == i || *s == v || body.tail.is_some() {
        return Err("its body is not exactly `s = s + ints_get(v, i); i = i + 1;`");
    }
    let ExprKind::Arith {
        op: ArithOp::Add,
        lhs: acc,
        rhs: get,
    } = &add.kind
    else {
        return Err("its body does not add an element to an accumulator");
    };
    let ExprKind::Builtin {
        op: Intrinsic::IntsGet,
        args: gargs,
        callee_at,
        ..
    } = &get.kind
    else {
        return Err("its body does not add an element to an accumulator");
    };
    if local(acc) != Some(*s)
        || gargs.first().and_then(local) != Some(v)
        || gargs.get(1).and_then(local) != Some(i)
    {
        return Err("its body does not add `ints_get(v, i)` to an accumulator");
    }
    let ExprKind::Arith {
        op: ArithOp::Add,
        lhs: si,
        rhs: one,
    } = &step.kind
    else {
        return Err("its counter does not step by 1");
    };
    if local(si) != Some(i) || int(consts, one) != Some(1) {
        return Err("its counter does not step by 1");
    }
    let read = |l: LocalId, ty: Type, at: Span| Expr {
        kind: ExprKind::Local(l),
        ty: Some(ty),
        at,
    };
    let sum = Expr {
        kind: ExprKind::Builtin {
            op: Intrinsic::IntsSum,
            type_args: Vec::new(),
            args: vec![read(*s, Type::Int, acc.at), read(v, Type::Ints, get.at)],
            callee_at: *callee_at,
        },
        ty: Some(Type::Int),
        // The addition's span: an overflow is reported where the loop reported it.
        at: add.at,
    };
    let len = Expr {
        kind: ExprKind::Builtin {
            op: Intrinsic::IntsLen,
            type_args: Vec::new(),
            args: vec![read(v, Type::Ints, rhs.at)],
            callee_at: rhs.at,
        },
        ty: Some(Type::Int),
        at: rhs.at,
    };
    Ok(vec![
        Stmt::Assign {
            local: *s,
            path: Vec::new(),
            value: sum,
            at: add.at,
        },
        Stmt::Assign {
            local: i,
            path: Vec::new(),
            value: len,
            at: step.at,
        },
    ])
}
