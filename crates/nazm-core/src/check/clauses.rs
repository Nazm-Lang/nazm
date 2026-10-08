//! Contract clauses and preconditions: the constant literals a precondition is proved from, and
//! how a clause is described.

use super::*;

/// A literal value, as the checker's precondition proof evaluates one (N87).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lit {
    Int(i64),
    Bool(bool),
}

impl Lit {
    pub(super) fn show(&self) -> String {
        match self {
            Self::Int(i) => i.to_string(),
            Self::Bool(b) => b.to_string(),
        }
    }
}

/// A place's path as a reader writes it: `b.c`, `items[…].x` (Gate 2 added indexes).
pub(super) fn shown_path(path: &[nazm_syntax::PlaceStep]) -> String {
    let mut out = String::new();
    for step in path {
        match step {
            nazm_syntax::PlaceStep::Field(f) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(&f.name);
            }
            nazm_syntax::PlaceStep::Index { .. } => out.push_str("[…]"),
        }
    }
    out
}

/// The conversion functions that are not type names (Gate 2), reserved as built-ins are.
pub(super) const CONVERSIONS: [&str; 3] = ["try_convert", "wrap_convert", "saturate_convert"];

/// Whether `e` is a number literal, possibly negated — the operand that takes its type from the
/// other side of a binary operator (Gate 2).
pub(super) fn is_number_literal(e: &Expr) -> bool {
    match e {
        Expr::Int { .. } | Expr::WideInt { .. } | Expr::Float { .. } => true,
        Expr::Neg { operand, .. } => is_number_literal(operand),
        _ => false,
    }
}

/// `e`'s value from literals and the names in `env`, or `None` for anything else — including
/// an operation that would fail, which is then not proved. `&&` and `||` short-circuit, as at
/// run time, so a clause guarded by one is evaluated as the program would evaluate it.
pub(super) fn literal(e: &Expr, env: &HashMap<&str, Lit>) -> Option<Lit> {
    match e {
        Expr::Int { value, .. } => Some(Lit::Int(*value)),
        Expr::Bool { value, .. } => Some(Lit::Bool(*value)),
        Expr::Var { name, .. } => env.get(name.as_str()).copied(),
        Expr::Neg { operand, .. } => match literal(operand, env)? {
            Lit::Int(i) => i.checked_neg().map(Lit::Int),
            Lit::Bool(_) => None,
        },
        Expr::Not { operand, .. } => match literal(operand, env)? {
            Lit::Bool(b) => Some(Lit::Bool(!b)),
            Lit::Int(_) => None,
        },
        Expr::Binary { op, lhs, rhs, .. } => {
            let l = literal(lhs, env)?;
            match (op, l) {
                (BinOp::And, Lit::Bool(false)) => return Some(Lit::Bool(false)),
                (BinOp::Or, Lit::Bool(true)) => return Some(Lit::Bool(true)),
                _ => {}
            }
            let r = literal(rhs, env)?;
            match (l, r) {
                (Lit::Int(a), Lit::Int(b)) => match op {
                    BinOp::Add => a.checked_add(b).map(Lit::Int),
                    BinOp::Sub => a.checked_sub(b).map(Lit::Int),
                    BinOp::Mul => a.checked_mul(b).map(Lit::Int),
                    BinOp::Div => a.checked_div(b).map(Lit::Int),
                    BinOp::Rem => a.checked_rem(b).map(Lit::Int),
                    BinOp::Eq => Some(Lit::Bool(a == b)),
                    BinOp::NotEq => Some(Lit::Bool(a != b)),
                    BinOp::Lt => Some(Lit::Bool(a < b)),
                    BinOp::LtEq => Some(Lit::Bool(a <= b)),
                    BinOp::Gt => Some(Lit::Bool(a > b)),
                    BinOp::GtEq => Some(Lit::Bool(a >= b)),
                    BinOp::BitAnd => Some(Lit::Int(a & b)),
                    BinOp::BitOr => Some(Lit::Int(a | b)),
                    BinOp::BitXor => Some(Lit::Int(a ^ b)),
                    BinOp::Shl => u32::try_from(b)
                        .ok()
                        .filter(|n| *n < 64)
                        .map(|n| Lit::Int(a << n)),
                    BinOp::Shr => u32::try_from(b)
                        .ok()
                        .filter(|n| *n < 64)
                        .map(|n| Lit::Int(a >> n)),
                    BinOp::And | BinOp::Or => None,
                },
                (Lit::Bool(a), Lit::Bool(b)) => match op {
                    BinOp::And => Some(Lit::Bool(a && b)),
                    BinOp::Or => Some(Lit::Bool(a || b)),
                    BinOp::Eq => Some(Lit::Bool(a == b)),
                    BinOp::NotEq => Some(Lit::Bool(a != b)),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// A clause, for a witness: its shape, not its source text, which the checker does not hold.
pub(super) fn describe_clause(e: &Expr) -> String {
    match e {
        Expr::Int { value, .. } => value.to_string(),
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Var { name, .. } => name.clone(),
        Expr::Neg { operand, .. } => format!("-{}", describe_clause(operand)),
        Expr::Not { operand, .. } => format!("!{}", describe_clause(operand)),
        Expr::Binary { op, lhs, rhs, .. } => {
            let o = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Eq => "==",
                BinOp::NotEq => "!=",
                BinOp::Lt => "<",
                BinOp::LtEq => "<=",
                BinOp::Gt => ">",
                BinOp::GtEq => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::BitAnd => "&",
                BinOp::BitOr => "|",
                BinOp::BitXor => "^",
                BinOp::Shl => "<<",
                BinOp::Shr => ">>",
            };
            format!("{} {o} {}", describe_clause(lhs), describe_clause(rhs))
        }
        _ => "…".to_owned(),
    }
}

/// How many `requires` clauses a function states (N87).
pub(super) fn preconditions_of(f: &Function) -> u32 {
    u32::try_from(
        f.contracts
            .iter()
            .filter(|c| c.kind == nazm_syntax::ContractKind::Requires)
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// The function a callee is, or a definition no function has.
pub(super) fn callee_fn(c: Callee) -> FnId {
    match c {
        Callee::Function(f) => f,
        Callee::Builtin(_) | Callee::Indirect(_) => FnId::from_index(usize::MAX),
    }
}
