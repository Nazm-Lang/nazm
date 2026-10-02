//! The formal core, `nazm.formal-core/1` (N61, `docs/formal-core.md`, `docs/architecture.md`
//! §7.63).
//!
//! The document is the semantics; this crate is it transcribed, one arm per rule, each arm named
//! by its rule. It depends on nothing of the compiler, so the compiler cannot be its own oracle:
//! `tests/exhaustive.rs` holds the compiler to it over every program up to a bound.

use std::fmt::Write as _;

/// An expression of the core.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Var(&'static str),
    Arith(Op, Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(&'static str, Box<Expr>, Box<Expr>),
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl Op {
    pub const ALL: [Self; 5] = [Self::Add, Self::Sub, Self::Mul, Self::Div, Self::Rem];

    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }
}

/// A type of the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Bool,
}

/// A value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

/// An outcome: a value, or a trap with its diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Value(Value),
    Trap(&'static str),
}

/// The overflow trap.
pub const OVERFLOW: &str = "N0400";
/// The division-by-zero trap.
pub const DIVIDE_BY_ZERO: &str = "N0401";

/// The type of `e` under `env`, by the typing rules; `None` when no rule applies.
#[must_use]
pub fn type_of(e: &Expr, env: &[(&'static str, Ty)]) -> Option<Ty> {
    match e {
        // T-Int, T-Bool
        Expr::Int(_) => Some(Ty::Int),
        Expr::Bool(_) => Some(Ty::Bool),
        // T-Var: the innermost binding of the name.
        Expr::Var(x) => env.iter().rev().find(|(n, _)| n == x).map(|(_, t)| *t),
        // T-Arith
        Expr::Arith(_, a, b) => {
            (type_of(a, env)? == Ty::Int && type_of(b, env)? == Ty::Int).then_some(Ty::Int)
        }
        // T-Lt
        Expr::Lt(a, b) => {
            (type_of(a, env)? == Ty::Int && type_of(b, env)? == Ty::Int).then_some(Ty::Bool)
        }
        // T-Eq
        Expr::Eq(a, b) => (type_of(a, env)? == type_of(b, env)?).then_some(Ty::Bool),
        // T-Not
        Expr::Not(a) => (type_of(a, env)? == Ty::Bool).then_some(Ty::Bool),
        // T-And
        Expr::And(a, b) => {
            (type_of(a, env)? == Ty::Bool && type_of(b, env)? == Ty::Bool).then_some(Ty::Bool)
        }
        // T-If
        Expr::If(c, a, b) => {
            if type_of(c, env)? != Ty::Bool {
                return None;
            }
            let t = type_of(a, env)?;
            (type_of(b, env)? == t).then_some(t)
        }
        // T-Let
        Expr::Let(x, a, b) => {
            let s = type_of(a, env)?;
            let mut inner = env.to_vec();
            inner.push((x, s));
            type_of(b, &inner)
        }
    }
}

/// `e` evaluated under `rho`, by the evaluation rules. `None` is *stuck*: no rule applies, which
/// soundness says never happens to a well-typed program.
#[must_use]
pub fn eval(e: &Expr, rho: &[(&'static str, Value)]) -> Option<Outcome> {
    let int = |o: Outcome| match o {
        Outcome::Value(Value::Int(n)) => Some(Ok(n)),
        Outcome::Trap(c) => Some(Err(c)),
        Outcome::Value(Value::Bool(_)) => None,
    };
    let boolean = |o: Outcome| match o {
        Outcome::Value(Value::Bool(b)) => Some(Ok(b)),
        Outcome::Trap(c) => Some(Err(c)),
        Outcome::Value(Value::Int(_)) => None,
    };
    Some(match e {
        // E-Lit
        Expr::Int(n) => Outcome::Value(Value::Int(*n)),
        Expr::Bool(b) => Outcome::Value(Value::Bool(*b)),
        // E-Var
        Expr::Var(x) => Outcome::Value(rho.iter().rev().find(|(n, _)| n == x)?.1),
        // E-Arith, E-Div, E-Trap: left to right, the first trap wins.
        Expr::Arith(op, a, b) => {
            let m = match int(eval(a, rho)?)? {
                Ok(m) => m,
                Err(c) => return Some(Outcome::Trap(c)),
            };
            let k = match int(eval(b, rho)?)? {
                Ok(k) => k,
                Err(c) => return Some(Outcome::Trap(c)),
            };
            arith(*op, m, k)
        }
        // E-Lt
        Expr::Lt(a, b) => {
            let m = match int(eval(a, rho)?)? {
                Ok(m) => m,
                Err(c) => return Some(Outcome::Trap(c)),
            };
            match int(eval(b, rho)?)? {
                Ok(k) => Outcome::Value(Value::Bool(m < k)),
                Err(c) => Outcome::Trap(c),
            }
        }
        // E-Eq
        Expr::Eq(a, b) => {
            let x = eval(a, rho)?;
            let Outcome::Value(x) = x else { return Some(x) };
            let y = eval(b, rho)?;
            let Outcome::Value(y) = y else { return Some(y) };
            match (x, y) {
                (Value::Int(m), Value::Int(k)) => Outcome::Value(Value::Bool(m == k)),
                (Value::Bool(m), Value::Bool(k)) => Outcome::Value(Value::Bool(m == k)),
                _ => return None,
            }
        }
        // E-Not
        Expr::Not(a) => match boolean(eval(a, rho)?)? {
            Ok(b) => Outcome::Value(Value::Bool(!b)),
            Err(c) => Outcome::Trap(c),
        },
        // E-And: `b` is evaluated only when `a` is true.
        Expr::And(a, b) => match boolean(eval(a, rho)?)? {
            Ok(false) => Outcome::Value(Value::Bool(false)),
            Ok(true) => match boolean(eval(b, rho)?)? {
                Ok(v) => Outcome::Value(Value::Bool(v)),
                Err(c) => Outcome::Trap(c),
            },
            Err(c) => Outcome::Trap(c),
        },
        // E-If: one branch only.
        Expr::If(c, a, b) => match boolean(eval(c, rho)?)? {
            Ok(true) => eval(a, rho)?,
            Ok(false) => eval(b, rho)?,
            Err(code) => Outcome::Trap(code),
        },
        // E-Let
        Expr::Let(x, a, b) => match eval(a, rho)? {
            Outcome::Value(v) => {
                let mut inner = rho.to_vec();
                inner.push((x, v));
                eval(b, &inner)?
            }
            t @ Outcome::Trap(_) => t,
        },
    })
}

/// E-Arith and E-Div on two integers.
#[must_use]
pub fn arith(op: Op, m: i64, k: i64) -> Outcome {
    let r = match op {
        Op::Add => m.checked_add(k),
        Op::Sub => m.checked_sub(k),
        Op::Mul => m.checked_mul(k),
        Op::Div | Op::Rem if k == 0 => return Outcome::Trap(DIVIDE_BY_ZERO),
        // Truncation toward zero; `i64::MIN / -1` does not fit.
        Op::Div => m.checked_div(k),
        // `i64::MIN % -1` is 0 — mathematically zero, and representable (`docs/spec.md`, the
        // arithmetic table). Rust's `checked_rem` reports that pair as overflow; the language
        // does not.
        Op::Rem if k == -1 => Some(0),
        Op::Rem => m.checked_rem(k),
    };
    r.map_or(Outcome::Trap(OVERFLOW), |n| Outcome::Value(Value::Int(n)))
}

/// The mathematical value of an arithmetic expression without traps, for P3: 128-bit, so every
/// `i64` operation's exact result fits.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "the language's `/` and `%` truncate toward zero"
)]
pub fn exact(op: Op, m: i64, k: i64) -> Option<i128> {
    let (m, k) = (i128::from(m), i128::from(k));
    match op {
        Op::Add => Some(m + k),
        Op::Sub => Some(m - k),
        Op::Mul => Some(m * k),
        Op::Div => (k != 0).then(|| m / k),
        Op::Rem => (k != 0).then(|| m % k),
    }
}

/// `e` as Nazm source: the body of `main`, whose result type is `t`.
#[must_use]
pub fn to_source(e: &Expr, t: Ty) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "fn main() -> {} {{ ",
        match t {
            Ty::Int => "Int",
            Ty::Bool => "Bool",
        }
    );
    write_expr(&mut s, e);
    s.push_str(" }\n");
    s
}

fn write_expr(s: &mut String, e: &Expr) {
    match e {
        Expr::Int(n) => {
            let _ = write!(s, "({n})");
        }
        Expr::Bool(b) => {
            let _ = write!(s, "{b}");
        }
        Expr::Var(x) => s.push_str(x),
        Expr::Arith(op, a, b) => {
            s.push('(');
            write_expr(s, a);
            let _ = write!(s, " {} ", op.symbol());
            write_expr(s, b);
            s.push(')');
        }
        Expr::Lt(a, b) | Expr::Eq(a, b) | Expr::And(a, b) => {
            let sym = match e {
                Expr::Lt(..) => "<",
                Expr::Eq(..) => "==",
                _ => "&&",
            };
            s.push('(');
            write_expr(s, a);
            let _ = write!(s, " {sym} ");
            write_expr(s, b);
            s.push(')');
        }
        Expr::Not(a) => {
            s.push_str("(!");
            write_expr(s, a);
            s.push(')');
        }
        Expr::If(c, a, b) => {
            s.push_str("(if ");
            write_expr(s, c);
            s.push_str(" { ");
            write_expr(s, a);
            s.push_str(" } else { ");
            write_expr(s, b);
            s.push_str(" })");
        }
        Expr::Let(x, a, b) => {
            let _ = write!(s, "{{ let {x} = ");
            write_expr(s, a);
            s.push_str("; ");
            write_expr(s, b);
            s.push_str(" }");
        }
    }
}

/// The literals the enumeration uses: zero, one, minus one, an odd number, and the extremes.
pub const INTS: [i64; 6] = [0, 1, -1, 7, i64::MAX, i64::MIN];

/// Every expression of exactly `size` nodes over `INTS`, both booleans and the names in `scope`,
/// each `let` binding the next of `x`, `y`, `z`. Ill-typed ones included.
#[must_use]
pub fn enumerate(size: usize, scope: &[&'static str]) -> Vec<Expr> {
    const NAMES: [&str; 3] = ["x", "y", "z"];
    let mut out = Vec::new();
    if size == 0 {
        return out;
    }
    if size == 1 {
        out.extend(INTS.iter().map(|n| Expr::Int(*n)));
        out.extend([Expr::Bool(false), Expr::Bool(true)]);
        out.extend(scope.iter().map(|x| Expr::Var(x)));
        return out;
    }
    let sub = size - 1;
    for a in enumerate(sub, scope) {
        out.push(Expr::Not(Box::new(a)));
    }
    for left in 1..sub {
        let right = sub - left;
        let la = enumerate(left, scope);
        let rb = enumerate(right, scope);
        for a in &la {
            for b in &rb {
                for op in Op::ALL {
                    out.push(Expr::Arith(op, Box::new(a.clone()), Box::new(b.clone())));
                }
                out.push(Expr::Lt(Box::new(a.clone()), Box::new(b.clone())));
                out.push(Expr::Eq(Box::new(a.clone()), Box::new(b.clone())));
                out.push(Expr::And(Box::new(a.clone()), Box::new(b.clone())));
            }
        }
        if let Some(x) = NAMES.get(scope.len()).copied() {
            let mut inner = scope.to_vec();
            inner.push(x);
            for a in &la {
                for b in enumerate(right, &inner) {
                    out.push(Expr::Let(x, Box::new(a.clone()), Box::new(b)));
                }
            }
        }
    }
    for c in 1..sub {
        for a in 1..sub - c {
            let b = sub - c - a;
            for ce in enumerate(c, scope) {
                for ae in enumerate(a, scope) {
                    for be in enumerate(b, scope) {
                        out.push(Expr::If(
                            Box::new(ce.clone()),
                            Box::new(ae.clone()),
                            Box::new(be),
                        ));
                    }
                }
            }
        }
    }
    out
}
