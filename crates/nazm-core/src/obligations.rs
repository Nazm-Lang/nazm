//! The obligation census (N87, `docs/architecture.md` §7.88): every place a checked program may
//! fail, or promise something, and whether that is proved, checked as the program runs, or
//! unknown.
//!
//! Read off Core IR, the representation every execution path starts from, so the census and
//! what runs cannot describe two different programs. Nothing here decides anything: a contract
//! clause, an operation that can overflow or divide by zero, an index, an allocation, a call of
//! a function stating a precondition, a call through a function value and a call into C are
//! each *found*, and their status is a fact the checker already recorded — a precondition proved
//! for a call's literal arguments — or the language's — every operation is checked — or
//! nobody's, for what is decided at run time by code this compiler does not see.

use nazm_cir as cir;
use nazm_sema::resolve::Resolution;
use nazm_sema::{Intrinsic, Type};
use nazm_span::Span;

use cir::{ArithOp, Block, Const, ConstId, Expr, ExprKind, Stmt};

/// What an obligation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Precondition,
    Postcondition,
    CallPrecondition,
    Overflow,
    Division,
    Index,
    Allocation,
    IndirectCall,
    ForeignCall,
    /// A shift or rotation's amount (Gate 2), and a conversion that must be exact.
    Shift,
    Conversion,
}

impl Kind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Precondition => "precondition",
            Self::Postcondition => "postcondition",
            Self::CallPrecondition => "call-precondition",
            Self::Overflow => "overflow",
            Self::Division => "division",
            Self::Index => "index",
            Self::Allocation => "allocation",
            Self::IndirectCall => "indirect-call",
            Self::ForeignCall => "foreign-call",
            Self::Shift => "shift",
            Self::Conversion => "conversion",
        }
    }
}

/// What is known of an obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Discharged at compile time; the obligation carries its witness.
    Proved,
    /// Checked when the program runs, which every Nazm operation is.
    Checked,
    /// Decided by code this compiler does not see: a function value's or C's.
    Unknown,
}

impl Status {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Proved => "proved",
            Self::Checked => "checked",
            Self::Unknown => "unknown",
        }
    }
}

/// One obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    /// The function it is in, by name, for a reader.
    pub function: String,
    pub kind: Kind,
    pub status: Status,
    pub at: Span,
    /// Why it is proved, for a proved one.
    pub witness: Option<String>,
}

/// Every obligation of `core`, function by function in Core IR's order, then by position.
#[must_use]
pub fn census(core: &cir::Program, res: &Resolution) -> Vec<Obligation> {
    let mut out = Vec::new();
    for f in &core.functions {
        // A foreign declaration has no body; its calls are the obligation, at the caller.
        if f.foreign.is_some() {
            continue;
        }
        let mut c = Census {
            res,
            function: &f.name,
            consts: &f.consts,
            out: Vec::new(),
        };
        c.block(&f.body);
        c.out.sort_by_key(|o| (o.at.start, o.at.end, o.kind));
        out.extend(c.out);
    }
    out
}

struct Census<'a> {
    res: &'a Resolution,
    function: &'a str,
    consts: &'a [Const],
    out: Vec<Obligation>,
}

impl Census<'_> {
    fn push(&mut self, kind: Kind, status: Status, at: Span, witness: Option<String>) {
        self.out.push(Obligation {
            function: self.function.to_owned(),
            kind,
            status,
            at,
            witness,
        });
    }

    fn int(&self, e: &Expr) -> Option<i64> {
        match e.kind {
            ExprKind::Const(ConstId(i)) => match self.consts.get(i as usize)? {
                Const::Int(v) => Some(*v),
                _ => None,
            },
            _ => None,
        }
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { value, .. } | Stmt::Assign { value, .. } | Stmt::Return { value, .. } => {
                self.expr(value);
            }
            Stmt::Eval(e) => self.expr(e),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.block(o);
                }
            }
            Stmt::Match(m) => self.matching(m),
            Stmt::Loop { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::Block(b) | Stmt::Scope { body: b, .. } => self.block(b),
            Stmt::Spawn { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    fn matching(&mut self, m: &cir::Match) {
        self.expr(&m.scrutinee);
        for a in &m.arms {
            self.block(&a.body);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Const(_) | ExprKind::Local(_) | ExprKind::FnValue(_) => {}
            // A float's negation, arithmetic and conversion to another float never fail (Gate 2).
            ExprKind::Neg(x) | ExprKind::Complement(x) if e.ty.is_some_and(Type::is_float) => {
                self.expr(x);
            }
            ExprKind::Arith { lhs, rhs, .. } if e.ty.is_some_and(Type::is_float) => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Complement(x) => self.expr(x),
            ExprKind::Neg(x) if e.ty != Some(Type::Int) => {
                self.push(Kind::Overflow, Status::Checked, e.at, None);
                self.expr(x);
            }
            ExprKind::Bits { op, lhs, rhs } => {
                if matches!(op, nazm_cir::BitOp::Shl | nazm_cir::BitOp::Shr) {
                    self.push(Kind::Shift, Status::Checked, e.at, None);
                }
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Convert { mode, operand } => {
                let exact =
                    *mode == nazm_sema::ConvertMode::Exact && !e.ty.is_some_and(Type::is_float);
                if exact {
                    self.push(Kind::Conversion, Status::Checked, e.at, None);
                }
                self.expr(operand);
            }
            ExprKind::Fits { operand, .. } => self.expr(operand),
            ExprKind::Array { elems } => {
                for x in elems {
                    self.expr(x);
                }
            }
            ExprKind::ArrayRepeat { value, .. } => self.expr(value),
            // An index is checked against its array's length when it runs (Gate 2).
            ExprKind::Index { base, index } => {
                self.push(Kind::Index, Status::Checked, e.at, None);
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Neg(x) => {
                // Only `Int`'s minimum cannot be negated.
                match self.int(x) {
                    Some(v) if v != i64::MIN => self.push(
                        Kind::Overflow,
                        Status::Proved,
                        e.at,
                        Some(format!("-({v}) is {}", -v)),
                    ),
                    _ => self.push(Kind::Overflow, Status::Checked, e.at, None),
                }
                self.expr(x);
            }
            ExprKind::Arith { op, lhs, rhs } => {
                self.arith(*op, lhs, rhs, e.at);
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Compare { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call {
                callee,
                args,
                callee_at,
                ..
            } => {
                let foreign = self
                    .res
                    .function(*callee)
                    .is_some_and(|d| d.foreign.is_some());
                if foreign {
                    self.push(Kind::ForeignCall, Status::Unknown, e.at, None);
                }
                let preconditions = self
                    .res
                    .checked_call(*callee_at)
                    .map_or(0, |c| c.preconditions);
                if preconditions > 0 {
                    match self.res.proved_call(*callee_at) {
                        Some(w) => self.push(
                            Kind::CallPrecondition,
                            Status::Proved,
                            e.at,
                            Some(w.to_owned()),
                        ),
                        None => self.push(Kind::CallPrecondition, Status::Checked, e.at, None),
                    }
                }
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::CallIndirect { callee, args, .. } => {
                self.push(Kind::IndirectCall, Status::Unknown, e.at, None);
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::CallTrait { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Builtin { op, args, .. } => {
                if *op == Intrinsic::ContractCheck {
                    // Which side is the second argument's constant: 0 a precondition. A
                    // postcondition is checked at every return, and is one obligation.
                    let kind = match args.get(1).and_then(|w| self.int(w)) {
                        Some(0) => Kind::Precondition,
                        _ => Kind::Postcondition,
                    };
                    if !self.out.iter().any(|o| o.at == e.at && o.kind == kind) {
                        self.push(kind, Status::Checked, e.at, None);
                    }
                } else {
                    self.builtin(*op, e.at);
                }
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Record { fields, .. } | ExprKind::Variant { fields, .. } => {
                for i in fields {
                    self.expr(&i.value);
                }
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Match(m) => self.matching(m),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                self.block(otherwise);
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Closure { .. } => {
                self.push(Kind::Allocation, Status::Checked, e.at, None);
            }
        }
    }

    /// An operation on two literals is proved by computing it; anything else is checked.
    fn arith(&mut self, op: ArithOp, lhs: &Expr, rhs: &Expr, at: Span) {
        let (l, r) = (self.int(lhs), self.int(rhs));
        match op {
            ArithOp::Add | ArithOp::Sub | ArithOp::Mul => {
                let computed = match (l, r) {
                    (Some(a), Some(b)) => match op {
                        ArithOp::Add => a.checked_add(b),
                        ArithOp::Sub => a.checked_sub(b),
                        _ => a.checked_mul(b),
                    },
                    _ => None,
                };
                match (computed, l, r) {
                    (Some(v), Some(a), Some(b)) => {
                        let sign = match op {
                            ArithOp::Add => "+",
                            ArithOp::Sub => "-",
                            _ => "*",
                        };
                        self.push(
                            Kind::Overflow,
                            Status::Proved,
                            at,
                            Some(format!("{a} {sign} {b} is {v}")),
                        );
                    }
                    _ => self.push(Kind::Overflow, Status::Checked, at, None),
                }
            }
            ArithOp::Div | ArithOp::Rem => {
                // A divisor other than 0 and -1 can neither be zero nor overflow `MIN / -1`.
                match r {
                    Some(b) if b != 0 && b != -1 => self.push(
                        Kind::Division,
                        Status::Proved,
                        at,
                        Some(format!("the divisor is {b}")),
                    ),
                    _ => self.push(Kind::Division, Status::Checked, at, None),
                }
            }
        }
    }

    fn builtin(&mut self, op: Intrinsic, at: Span) {
        let kind = match op {
            Intrinsic::StrByte
            | Intrinsic::StrSlice
            | Intrinsic::IntsGet
            | Intrinsic::IntsSet
            | Intrinsic::StrsGet
            | Intrinsic::StrsSet
            | Intrinsic::VecGet
            | Intrinsic::VecSet
            | Intrinsic::Arg
            | Intrinsic::StrFromByte
            | Intrinsic::IntsPop
            | Intrinsic::StrsPop
            | Intrinsic::VecPop => Kind::Index,
            Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrJoin
            | Intrinsic::IntsNew
            | Intrinsic::StrsNew
            | Intrinsic::VecNew
            | Intrinsic::IntsPush
            | Intrinsic::StrsPush
            | Intrinsic::VecPush
            | Intrinsic::ChanNew
            | Intrinsic::ChanNewOf
            | Intrinsic::AtomicNew
            | Intrinsic::ReadFile
            | Intrinsic::CStrCopy => Kind::Allocation,
            Intrinsic::IntsSum | Intrinsic::AtomicAdd => Kind::Overflow,
            _ => return,
        };
        self.push(kind, Status::Checked, at, None);
    }
}
