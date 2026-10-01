//! Resource facts, read off MIR (N52, `docs/architecture.md` §7.54).
//!
//! Every operation that allocates, retains, blocks, talks to a channel, calls C or starts a task
//! is already an explicit MIR statement, so the facts are one pass over each function's blocks
//! and nothing is rediscovered. Each site carries a count **per call of its function**:
//! [`Count::AtMostOnce`] when its block lies on no cycle of the control-flow graph, and
//! [`Count::Unknown`] when it does — a loop runs as many times as values the compiler does not
//! know say. There is no zero: a site that exists may run. Nothing here is a measurement.

use crate::ir::{Block, BlockId, FuncId, Function, Program, Rvalue, Statement, StatementKind};
use nazm_sema::Intrinsic;
use nazm_span::Span;

/// What a site does with resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SiteKind {
    /// Makes storage: a sequence, a string, a channel, a closure object, or a push that may grow
    /// its sequence.
    Allocation,
    /// Takes a new reference to managed storage that already exists.
    Retain,
    /// May wait: a send or receive on a bounded channel, or a `scope`'s join.
    Block,
    /// A channel operation.
    Channel,
    /// A call to C.
    Foreign,
    /// Starts a task.
    Task,
}

impl SiteKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Allocation => "allocation",
            Self::Retain => "retain",
            Self::Block => "block",
            Self::Channel => "channel",
            Self::Foreign => "foreign",
            Self::Task => "task",
        }
    }
}

/// How many times a site may run per call of its function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Count {
    /// Its block is on no cycle: at most once — the site is a static fact, the bound is one.
    AtMostOnce,
    /// Its block is on a cycle: a count the compiler does not know. Never written as a number.
    Unknown,
}

impl Count {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AtMostOnce => "at-most-once",
            Self::Unknown => "unknown",
        }
    }
}

/// One site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub kind: SiteKind,
    /// What the operation is: a built-in's name, `closure`, `copy`, `spawn`, `join`, or the
    /// foreign function's name.
    pub op: String,
    pub at: Span,
    pub count: Count,
    /// Whether the site is on a failure path only — a compiler-made cleanup block, whose
    /// position is the failure's, not a line of source.
    pub on_failure: bool,
}

/// A function's sites, in block and statement order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCost {
    pub func: FuncId,
    pub sites: Vec<Site>,
}

/// Every concrete function's sites, in [`FuncId`] order.
#[must_use]
pub fn of(program: &Program) -> Vec<FunctionCost> {
    program
        .functions
        .iter()
        .map(|f| FunctionCost {
            func: f.id,
            sites: sites(program, f),
        })
        .collect()
}

fn sites(program: &Program, f: &Function) -> Vec<Site> {
    let cyclic = on_cycles(&f.blocks);
    let mut out = Vec::new();
    for (i, b) in f.blocks.iter().enumerate() {
        let count = if cyclic.get(i).copied().unwrap_or(true) {
            Count::Unknown
        } else {
            Count::AtMostOnce
        };
        let on_failure = !matches!(b.origin, crate::ir::BlockOrigin::Source);
        for s in &b.stmts {
            statement(program, f, s, count, on_failure, &mut out);
        }
    }
    out
}

fn statement(
    program: &Program,
    f: &Function,
    s: &Statement,
    count: Count,
    on_failure: bool,
    out: &mut Vec<Site>,
) {
    let mut push = |kind: SiteKind, op: &str| {
        out.push(Site {
            kind,
            op: op.to_owned(),
            at: s.at,
            count,
            on_failure,
        });
    };
    let managed = |l: crate::ir::LocalId| {
        f.local(l)
            .and_then(|d| d.ty.value())
            .is_some_and(|t| program.types.managed(t))
    };
    match &s.kind {
        StatementKind::Assign { dst, value, .. } => match value {
            Rvalue::Primitive { op, .. } => {
                if allocates(*op) {
                    push(SiteKind::Allocation, op.name());
                }
                if matches!(op, Intrinsic::StrSlice) {
                    push(SiteKind::Retain, op.name());
                }
                if matches!(
                    op,
                    Intrinsic::ChanNew
                        | Intrinsic::ChanSend
                        | Intrinsic::ChanRecv
                        | Intrinsic::ChanClose
                ) {
                    push(SiteKind::Channel, op.name());
                }
                if matches!(op, Intrinsic::ChanSend | Intrinsic::ChanRecv) {
                    push(SiteKind::Block, op.name());
                }
            }
            Rvalue::Closure { .. } => push(SiteKind::Allocation, "closure"),
            Rvalue::Copy(l) if managed(*l) => push(SiteKind::Retain, "copy"),
            Rvalue::Field { .. } | Rvalue::Payload { .. } | Rvalue::Capture { .. }
                if managed(*dst) =>
            {
                push(SiteKind::Retain, "copy");
            }
            Rvalue::Call { callee, .. } => {
                if let Some(g) = program.function(*callee) {
                    if g.foreign.is_some() {
                        push(SiteKind::Foreign, &g.name);
                    }
                }
            }
            _ => {}
        },
        StatementKind::Spawn { .. } => push(SiteKind::Task, "spawn"),
        StatementKind::ScopeJoin { .. } => push(SiteKind::Block, "join"),
        _ => {}
    }
}

/// Whether a built-in makes storage — a push counted, since it may grow its sequence.
const fn allocates(op: Intrinsic) -> bool {
    matches!(
        op,
        Intrinsic::IntsNew
            | Intrinsic::StrsNew
            | Intrinsic::VecNew
            | Intrinsic::ChanNew
            | Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrFromByte
            | Intrinsic::StrJoin
            | Intrinsic::ReadFile
            | Intrinsic::Arg
            | Intrinsic::IntsPush
            | Intrinsic::StrsPush
            | Intrinsic::VecPush
    )
}

/// Which blocks lie on a cycle of the control-flow graph, failure edges included: Tarjan's
/// algorithm with an explicit stack, so a large function costs heap and not this stack.
fn on_cycles(blocks: &[Block]) -> Vec<bool> {
    let n = blocks.len();
    let edges: Vec<Vec<usize>> = blocks
        .iter()
        .map(|b| {
            b.successors()
                .into_iter()
                .map(BlockId::index)
                .filter(|&j| j < n)
                .collect()
        })
        .collect();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut out = vec![false; n];
    let mut next = 0usize;
    for root in 0..n {
        if index.get(root).copied() != Some(usize::MAX) {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, i)) = work.last() {
            if i == 0 {
                if let (Some(x), Some(l)) = (index.get_mut(v), low.get_mut(v)) {
                    *x = next;
                    *l = next;
                }
                next += 1;
                stack.push(v);
                if let Some(o) = on_stack.get_mut(v) {
                    *o = true;
                }
            }
            if let Some(&w) = edges.get(v).and_then(|e| e.get(i)) {
                if let Some(top) = work.last_mut() {
                    top.1 = i + 1;
                }
                if index.get(w).copied() == Some(usize::MAX) {
                    work.push((w, 0));
                } else if on_stack.get(w).copied().unwrap_or(false) {
                    let iw = index.get(w).copied().unwrap_or(0);
                    if let Some(l) = low.get_mut(v) {
                        *l = (*l).min(iw);
                    }
                }
                continue;
            }
            work.pop();
            let lv = low.get(v).copied().unwrap_or(0);
            if let Some(&(parent, _)) = work.last() {
                if let Some(l) = low.get_mut(parent) {
                    *l = (*l).min(lv);
                }
            }
            if lv == index.get(v).copied().unwrap_or(0) {
                let mut members = Vec::new();
                while let Some(w) = stack.pop() {
                    if let Some(o) = on_stack.get_mut(w) {
                        *o = false;
                    }
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                let cycle = members.len() > 1 || edges.get(v).is_some_and(|e| e.contains(&v));
                if cycle {
                    for m in members {
                        if let Some(o) = out.get_mut(m) {
                            *o = true;
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BlockOrigin, Terminator};

    fn block(term: Terminator) -> Block {
        Block {
            stmts: Vec::new(),
            term,
            term_at: Span::synthetic(),
            origin: BlockOrigin::Source,
        }
    }

    #[test]
    fn a_loop_s_blocks_are_on_a_cycle_and_nothing_else_is() {
        // 0 → 1 ⇄ 2, 1 → 3: the loop is 1 and 2.
        let blocks = vec![
            block(Terminator::Goto(BlockId(1))),
            block(Terminator::Branch {
                cond: crate::ir::Operand::Const(crate::ir::Const::Bool(true)),
                then: BlockId(2),
                otherwise: BlockId(3),
            }),
            block(Terminator::Goto(BlockId(1))),
            block(Terminator::Unwind),
        ];
        assert_eq!(on_cycles(&blocks), [false, true, true, false]);
        let lone = vec![block(Terminator::Goto(BlockId(0)))];
        assert_eq!(on_cycles(&lone), [true]);
    }
}
