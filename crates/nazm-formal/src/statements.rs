//! The statement core, `nazm.formal-core/2` (N97, `docs/internal/formal-core.md` §2, `docs/internal/architecture.md`
//! §7.98): mutable bindings, assignment, `while`, `if` as a statement, `return`, and one recursive
//! function — over `Int`, with the expression core's arithmetic and its traps.
//!
//! Evaluation is big-step with **fuel**: each loop turn and each call spends one, and running out is
//! `Outcome2::OutOfFuel` — not a trap and not a value, but the semantics declining to say, because
//! Nazm bounds neither iteration nor recursion. Every property is stated for programs that finish
//! within the fuel, and the enumeration reports how many do not.

use std::fmt::Write as _;

use crate::{arith, Op, Outcome, Value};

/// An `Int` expression: literals, names, arithmetic, and a call of the one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum E {
    Int(i64),
    Var(&'static str),
    Arith(Op, Box<E>, Box<E>),
    Call(Box<E>),
}

/// A condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cond {
    Lt(E, E),
    Eq(E, E),
    Not(Box<Cond>),
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `x = e;` — `x` must be a mutable binding in scope.
    Assign(&'static str, E),
    While(Cond, Vec<Stmt>),
    If(Cond, Vec<Stmt>, Vec<Stmt>),
    Return(E),
}

/// A body: `let mut` bindings in order, statements, and the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub lets: Vec<(&'static str, E)>,
    pub stmts: Vec<Stmt>,
    pub tail: E,
}

/// A program: `main`, and optionally `fn f(n: Int) -> Int`, which may call itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub f: Option<Body>,
    pub main: Body,
}

/// How a program ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome2 {
    Done(Outcome),
    /// The fuel ran out: no claim is made.
    OutOfFuel,
}

/// The fuel a program starts with: loop turns and calls together.
pub const FUEL: u32 = 400;

/// W-Prog: every name used is bound, every assigned name is a mutable binding, and a call is made
/// only when `f` exists. All values are `Int`, so being well formed is being well typed.
#[must_use]
pub fn well_formed(p: &Program) -> bool {
    fn expr(e: &E, scope: &[&str], has_f: bool) -> bool {
        match e {
            E::Int(_) => true,
            E::Var(x) => scope.contains(x),
            E::Arith(_, a, b) => expr(a, scope, has_f) && expr(b, scope, has_f),
            E::Call(a) => has_f && expr(a, scope, has_f),
        }
    }
    fn cond(c: &Cond, scope: &[&str], has_f: bool) -> bool {
        match c {
            Cond::Lt(a, b) | Cond::Eq(a, b) => expr(a, scope, has_f) && expr(b, scope, has_f),
            Cond::Not(c) => cond(c, scope, has_f),
        }
    }
    fn stmts(ss: &[Stmt], scope: &[&str], mutable: &[&str], has_f: bool) -> bool {
        ss.iter().all(|s| match s {
            Stmt::Assign(x, e) => mutable.contains(x) && expr(e, scope, has_f),
            Stmt::While(c, b) => cond(c, scope, has_f) && stmts(b, scope, mutable, has_f),
            Stmt::If(c, a, b) => {
                cond(c, scope, has_f)
                    && stmts(a, scope, mutable, has_f)
                    && stmts(b, scope, mutable, has_f)
            }
            Stmt::Return(e) => expr(e, scope, has_f),
        })
    }
    fn body(b: &Body, params: &[&'static str], has_f: bool) -> bool {
        let mut scope: Vec<&str> = params.to_vec();
        let mut mutable = Vec::new();
        for (x, e) in &b.lets {
            if !expr(e, &scope, has_f) || scope.contains(x) {
                return false;
            }
            scope.push(x);
            mutable.push(*x);
        }
        stmts(&b.stmts, &scope, &mutable, has_f) && expr(&b.tail, &scope, has_f)
    }
    p.f.as_ref().is_none_or(|f| body(f, &["n"], true)) && body(&p.main, &[], p.f.is_some())
}

/// Control leaving a statement.
enum Flow {
    Next,
    Return(i64),
    Trap(&'static str),
    OutOfFuel,
}

struct Machine<'a> {
    f: Option<&'a Body>,
    fuel: u32,
}

impl Machine<'_> {
    /// S-Expr: left to right, the first trap wins; a call spends one fuel (S-Call).
    fn expr(&mut self, e: &E, rho: &[(&'static str, i64)]) -> Option<Result<i64, Flow>> {
        Some(match e {
            E::Int(n) => Ok(*n),
            E::Var(x) => Ok(rho.iter().rev().find(|(n, _)| n == x)?.1),
            E::Arith(op, a, b) => {
                let m = match self.expr(a, rho)? {
                    Ok(m) => m,
                    err => return Some(err),
                };
                let k = match self.expr(b, rho)? {
                    Ok(k) => k,
                    err => return Some(err),
                };
                match arith(*op, m, k) {
                    Outcome::Value(Value::Int(v)) => Ok(v),
                    Outcome::Trap(c) => Err(Flow::Trap(c)),
                    Outcome::Value(Value::Bool(_)) => return None,
                }
            }
            E::Call(a) => {
                let arg = match self.expr(a, rho)? {
                    Ok(v) => v,
                    err => return Some(err),
                };
                if self.fuel == 0 {
                    return Some(Err(Flow::OutOfFuel));
                }
                self.fuel -= 1;
                let f = self.f?;
                match self.body(f, &[("n", arg)])? {
                    Ok(v) => Ok(v),
                    Err(flow) => Err(flow),
                }
            }
        })
    }

    fn cond(&mut self, c: &Cond, rho: &[(&'static str, i64)]) -> Option<Result<bool, Flow>> {
        Some(match c {
            Cond::Lt(a, b) | Cond::Eq(a, b) => {
                let m = match self.expr(a, rho)? {
                    Ok(m) => m,
                    Err(f) => return Some(Err(f)),
                };
                let k = match self.expr(b, rho)? {
                    Ok(k) => k,
                    Err(f) => return Some(Err(f)),
                };
                Ok(if matches!(c, Cond::Lt(..)) {
                    m < k
                } else {
                    m == k
                })
            }
            Cond::Not(c) => self.cond(c, rho)?.map(|b| !b),
        })
    }

    /// S-Assign, S-While (one fuel per turn), S-If, S-Return.
    fn stmts(&mut self, ss: &[Stmt], rho: &mut Vec<(&'static str, i64)>) -> Option<Flow> {
        for s in ss {
            let flow = match s {
                Stmt::Assign(x, e) => match self.expr(e, rho)? {
                    Ok(v) => {
                        let slot = rho.iter_mut().rev().find(|(n, _)| n == x)?;
                        slot.1 = v;
                        Flow::Next
                    }
                    Err(f) => f,
                },
                Stmt::While(c, body) => loop {
                    match self.cond(c, rho)? {
                        Ok(false) => break Flow::Next,
                        Ok(true) => {}
                        Err(f) => break f,
                    }
                    if self.fuel == 0 {
                        break Flow::OutOfFuel;
                    }
                    self.fuel -= 1;
                    match self.stmts(body, rho)? {
                        Flow::Next => {}
                        other => break other,
                    }
                },
                Stmt::If(c, a, b) => match self.cond(c, rho)? {
                    Ok(true) => self.stmts(a, rho)?,
                    Ok(false) => self.stmts(b, rho)?,
                    Err(f) => f,
                },
                Stmt::Return(e) => match self.expr(e, rho)? {
                    Ok(v) => Flow::Return(v),
                    Err(f) => f,
                },
            };
            if !matches!(flow, Flow::Next) {
                return Some(flow);
            }
        }
        Some(Flow::Next)
    }

    /// S-Body: the bindings in order, the statements, then the result — unless a statement
    /// returned, trapped or ran out of fuel.
    fn body(&mut self, b: &Body, params: &[(&'static str, i64)]) -> Option<Result<i64, Flow>> {
        let mut rho: Vec<(&'static str, i64)> = params.to_vec();
        for (x, e) in &b.lets {
            match self.expr(e, &rho)? {
                Ok(v) => rho.push((x, v)),
                Err(f) => return Some(Err(f)),
            }
        }
        Some(match self.stmts(&b.stmts, &mut rho)? {
            Flow::Next => return self.expr(&b.tail, &rho),
            Flow::Return(v) => Ok(v),
            other => Err(other),
        })
    }
}

/// The program evaluated with [`FUEL`]; `None` is stuck, which a well-formed program never is.
#[must_use]
pub fn run(p: &Program) -> Option<Outcome2> {
    let mut m = Machine {
        f: p.f.as_ref(),
        fuel: FUEL,
    };
    Some(match m.body(&p.main, &[])? {
        Ok(v) => Outcome2::Done(Outcome::Value(Value::Int(v))),
        Err(Flow::Trap(c)) => Outcome2::Done(Outcome::Trap(c)),
        Err(Flow::OutOfFuel) => Outcome2::OutOfFuel,
        Err(Flow::Next | Flow::Return(_)) => return None,
    })
}

fn write_e(s: &mut String, e: &E) {
    match e {
        E::Int(n) => {
            let _ = write!(s, "({n})");
        }
        E::Var(x) => s.push_str(x),
        E::Arith(op, a, b) => {
            s.push('(');
            write_e(s, a);
            let _ = write!(s, " {} ", op.symbol());
            write_e(s, b);
            s.push(')');
        }
        E::Call(a) => {
            s.push_str("f(");
            write_e(s, a);
            s.push(')');
        }
    }
}

fn write_cond(s: &mut String, c: &Cond) {
    match c {
        Cond::Lt(a, b) | Cond::Eq(a, b) => {
            s.push('(');
            write_e(s, a);
            s.push_str(if matches!(c, Cond::Lt(..)) {
                " < "
            } else {
                " == "
            });
            write_e(s, b);
            s.push(')');
        }
        Cond::Not(c) => {
            s.push_str("(!");
            write_cond(s, c);
            s.push(')');
        }
    }
}

fn write_stmts(s: &mut String, ss: &[Stmt]) {
    for st in ss {
        match st {
            Stmt::Assign(x, e) => {
                let _ = write!(s, "{x} = ");
                write_e(s, e);
                s.push_str("; ");
            }
            Stmt::While(c, b) => {
                s.push_str("while ");
                write_cond(s, c);
                s.push_str(" { ");
                write_stmts(s, b);
                s.push_str("} ");
            }
            Stmt::If(c, a, b) => {
                s.push_str("if ");
                write_cond(s, c);
                s.push_str(" { ");
                write_stmts(s, a);
                s.push_str("} else { ");
                write_stmts(s, b);
                s.push_str("} ");
            }
            Stmt::Return(e) => {
                s.push_str("return ");
                write_e(s, e);
                s.push_str("; ");
            }
        }
    }
}

fn write_body(s: &mut String, b: &Body) {
    for (x, e) in &b.lets {
        let _ = write!(s, "let mut {x} = ");
        write_e(s, e);
        s.push_str("; ");
    }
    write_stmts(s, &b.stmts);
    write_e(s, &b.tail);
}

/// The program as Nazm source.
#[must_use]
pub fn to_source(p: &Program) -> String {
    let mut s = String::new();
    if let Some(f) = &p.f {
        s.push_str("fn f(n: Int) -> Int { ");
        write_body(&mut s, f);
        s.push_str(" }\n");
    }
    s.push_str("fn main() -> Int { ");
    write_body(&mut s, &p.main);
    s.push_str(" }\n");
    s
}

fn var(x: &'static str) -> E {
    E::Var(x)
}

fn op(o: Op, a: E, b: E) -> E {
    E::Arith(o, Box::new(a), Box::new(b))
}

/// The bound (§2 of `docs/internal/formal-core.md`): three families, every combination of each.
///
/// - **Loops**: `let mut x = a; let mut y = b; while C { x = E1; y = E2; } R`, with `a` and `b` from
///   the edges, six conditions, eight updates of `x`, five of `y` and three results.
/// - **Branches with early return**: `let mut x = a; if C { x = E1; } else { return E2; } x`.
/// - **Recursion**: `fn f(n) { if n < 1 { return B; } A }` with `A` built on `f(n - 1)`, called on
///   an argument from the edges.
#[must_use]
pub fn enumerate() -> Vec<Program> {
    let edges = [0, 1, 7, i64::MAX, i64::MIN];
    let x = || var("x");
    let y = || var("y");
    let conds = [
        Cond::Lt(x(), E::Int(10)),
        Cond::Lt(y(), E::Int(5)),
        Cond::Not(Box::new(Cond::Lt(x(), E::Int(100)))),
        Cond::Lt(E::Int(0), x()),
        Cond::Eq(y(), E::Int(0)),
        Cond::Lt(x(), y()),
    ];
    let x_updates = [
        op(Op::Add, x(), E::Int(1)),
        op(Op::Mul, x(), E::Int(2)),
        op(Op::Sub, x(), E::Int(1)),
        op(Op::Add, x(), y()),
        op(Op::Div, x(), E::Int(2)),
        op(Op::Rem, x(), E::Int(3)),
        op(Op::Div, E::Int(100), x()),
        op(Op::Sub, x(), E::Int(i64::MAX)),
    ];
    let y_updates = [
        op(Op::Add, y(), E::Int(1)),
        op(Op::Sub, y(), E::Int(1)),
        op(Op::Mul, y(), x()),
        E::Int(0),
        op(Op::Rem, x(), y()),
    ];
    let results = [x(), y(), op(Op::Sub, x(), y())];
    let mut out = Vec::new();
    for a in edges {
        for b in [0, 1, 3] {
            for c in &conds {
                for e1 in &x_updates {
                    for e2 in &y_updates {
                        for r in &results {
                            out.push(Program {
                                f: None,
                                main: Body {
                                    lets: vec![("x", E::Int(a)), ("y", E::Int(b))],
                                    stmts: vec![Stmt::While(
                                        c.clone(),
                                        vec![
                                            Stmt::Assign("x", e1.clone()),
                                            Stmt::Assign("y", e2.clone()),
                                        ],
                                    )],
                                    tail: r.clone(),
                                },
                            });
                        }
                    }
                }
            }
        }
    }
    for a in edges {
        for c in &conds {
            for e1 in &x_updates {
                for e2 in &x_updates {
                    out.push(Program {
                        f: None,
                        main: Body {
                            lets: vec![("x", E::Int(a)), ("y", E::Int(2))],
                            stmts: vec![Stmt::If(
                                c.clone(),
                                vec![Stmt::Assign("x", e1.clone())],
                                vec![Stmt::Return(e2.clone())],
                            )],
                            tail: x(),
                        },
                    });
                }
            }
        }
    }
    let n = || var("n");
    let rec = || E::Call(Box::new(op(Op::Sub, n(), E::Int(1))));
    let bases = [E::Int(0), E::Int(1), n(), E::Int(i64::MAX)];
    let steps = [
        op(Op::Add, rec(), E::Int(1)),
        op(Op::Mul, rec(), E::Int(2)),
        op(Op::Add, n(), rec()),
        op(Op::Sub, rec(), n()),
        op(Op::Mul, n(), rec()),
    ];
    for base in &bases {
        for step in &steps {
            for arg in [0, 1, 5, 10, 20, 25, 70, -3, 1000] {
                out.push(Program {
                    f: Some(Body {
                        lets: Vec::new(),
                        stmts: vec![Stmt::If(
                            Cond::Lt(n(), E::Int(1)),
                            vec![Stmt::Return(base.clone())],
                            Vec::new(),
                        )],
                        tail: step.clone(),
                    }),
                    main: Body {
                        lets: Vec::new(),
                        stmts: Vec::new(),
                        tail: E::Call(Box::new(E::Int(arg))),
                    },
                });
            }
        }
    }
    out
}
