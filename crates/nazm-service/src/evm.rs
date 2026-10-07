//! The EVM backend (N70, `docs/architecture.md` §7.72): a contract (§7.71) compiled to bytecode,
//! directly from Core IR.
//!
//! Values are 256-bit words holding sign-extended `i64`s, every arithmetic result checked with
//! `SIGNEXTEND(7, r) == r`. Locals live in memory, 32 bytes each from `0x80`; a pure helper is
//! inlined into a frame of its own (the `web3` profile makes the call graph acyclic). The state is
//! read with `SLOAD` at each field's name-derived slot and written with `SSTORE` when an entrypoint
//! returns `Ok`; where the contract declares `conserved`, the quantity is computed before and after
//! and a change reverts with `N0396`, as the simulator rejects it.

use std::collections::BTreeMap;

use nazm_core::cir::ir::{
    ArithOp, Block, CmpOp, Const, Expr, ExprKind, Function, LocalId, LoopId, Program, Stmt,
};
use nazm_core::sema::{FnId, ModuleId, Resolution, Type};
use nazm_span::Span;

use crate::contract::Contract;
use crate::keccak::keccak256;

/// Why the backend will not emit a contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub function: String,
    pub at: Span,
    pub why: String,
}

/// One entrypoint, as the EVM sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abi {
    pub name: String,
    pub signature: String,
    pub selector: [u8; 4],
    pub args: Vec<String>,
    /// A static upper bound on the body's execution gas, or `None` where a loop leaves it open.
    pub gas_bound: Option<u64>,
}

/// One state field's storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub field: String,
    pub ty: &'static str,
    pub slot: [u8; 32],
}

/// The compiled contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evm {
    /// The deployment code: `init()`'s state written, then the runtime returned.
    pub deploy: Vec<u8>,
    pub runtime: Vec<u8>,
    pub abi: Vec<Abi>,
    pub storage: Vec<Slot>,
}

/// The slot of state field `name`: `keccak256("nazm.state." + name)`.
#[must_use]
pub fn slot_of(name: &str) -> [u8; 32] {
    keccak256(format!("nazm.state.{name}").as_bytes())
}

#[derive(Debug, Clone)]
enum Item {
    Op(u8),
    Push(Vec<u8>),
    Label(usize),
    PushLabel(usize),
}

mod op {
    pub(super) const STOP: u8 = 0x00;
    pub(super) const ADD: u8 = 0x01;
    pub(super) const MUL: u8 = 0x02;
    pub(super) const SUB: u8 = 0x03;
    pub(super) const SDIV: u8 = 0x05;
    pub(super) const SMOD: u8 = 0x07;
    pub(super) const AND: u8 = 0x16;
    pub(super) const DUP3: u8 = 0x82;
    pub(super) const SWAP1: u8 = 0x90;
    pub(super) const SIGNEXTEND: u8 = 0x0b;
    pub(super) const LT: u8 = 0x10;
    pub(super) const SLT: u8 = 0x12;
    pub(super) const SGT: u8 = 0x13;
    pub(super) const EQ: u8 = 0x14;
    pub(super) const ISZERO: u8 = 0x15;
    pub(super) const SHR: u8 = 0x1c;
    pub(super) const CALLER: u8 = 0x33;
    pub(super) const CALLDATALOAD: u8 = 0x35;
    pub(super) const CALLDATASIZE: u8 = 0x36;
    pub(super) const CODECOPY: u8 = 0x39;
    pub(super) const POP: u8 = 0x50;
    pub(super) const MLOAD: u8 = 0x51;
    pub(super) const MSTORE: u8 = 0x52;
    pub(super) const SLOAD: u8 = 0x54;
    pub(super) const SSTORE: u8 = 0x55;
    pub(super) const JUMP: u8 = 0x56;
    pub(super) const JUMPI: u8 = 0x57;
    pub(super) const JUMPDEST: u8 = 0x5b;
    pub(super) const DUP1: u8 = 0x80;
    pub(super) const RETURN: u8 = 0xf3;
    pub(super) const REVERT: u8 = 0xfd;
}

/// The execution gas an opcode costs at least (Shanghai), memory and storage apart.
fn gas(o: u8) -> u64 {
    match o {
        op::STOP | op::RETURN | op::REVERT => 0,
        op::JUMPDEST => 1,
        op::CALLER | op::CALLDATASIZE | op::POP => 2,
        op::JUMP => 8,
        op::JUMPI => 10,
        op::MUL | op::SDIV | op::SMOD | op::SIGNEXTEND => 5,
        op::SLOAD => 2100,
        op::SSTORE => 22100,
        // The rest used here: arithmetic, comparison, shifts, `DUP`, memory — 3 each.
        _ => 3,
    }
}

/// What a state parameter is, in a frame: storage, or new values held in memory slots.
#[derive(Debug, Clone, Copy)]
enum View {
    Storage,
    Memory(usize),
}

#[derive(Debug, Clone)]
struct Frame<'a> {
    f: &'a Function,
    /// Memory slot of local 0 of this frame.
    base: usize,
    /// Where `return` writes, and the label it jumps to; `None` in an entrypoint, where it ends
    /// the transaction.
    ret: Option<(usize, usize)>,
    /// The stack height this frame's statements run at.
    floor: usize,
    state: Option<(LocalId, View)>,
    /// The caller's local, in an entrypoint.
    caller: Option<LocalId>,
    loops: BTreeMap<LoopId, (usize, usize, usize)>,
}

struct Gen<'a> {
    program: &'a Program,
    fields: &'a [String],
    state_ty: Type,
    items: Vec<Item>,
    labels: usize,
    next_slot: usize,
    height: usize,
    gas: u64,
    bounded: bool,
    depth: usize,
    fail_labels: BTreeMap<&'static str, usize>,
    conserved: Option<FnId>,
}

type R<T> = Result<T, Refusal>;

fn refuse(f: &Function, at: Span, why: &str) -> Refusal {
    Refusal {
        function: f.name.clone(),
        at,
        why: why.to_owned(),
    }
}

#[allow(clippy::indexing_slicing, reason = "a 32-byte word has bytes 24 to 31")]
fn int_word(n: i64) -> Vec<u8> {
    let mut w = if n < 0 { vec![0xff; 32] } else { vec![0; 32] };
    w[24..].copy_from_slice(&n.to_be_bytes());
    // The shortest push that sign-extends to the same word.
    let lead = w.iter().take_while(|&&b| b == 0).count();
    if lead == 32 {
        return vec![0];
    }
    w.get(lead..).map(<[u8]>::to_vec).unwrap_or_default()
}

impl<'a> Gen<'a> {
    fn op(&mut self, o: u8) {
        self.gas += gas(o);
        self.items.push(Item::Op(o));
        // The stack effect of the opcodes this backend emits.
        let (pops, pushes): (usize, usize) = match o {
            op::ADD
            | op::MUL
            | op::SUB
            | op::SDIV
            | op::SMOD
            | op::SIGNEXTEND
            | op::LT
            | op::SLT
            | op::SGT
            | op::EQ
            | op::SHR
            | op::AND => (2, 1),
            op::DUP3 => (0, 1),
            op::SWAP1 => (0, 0),
            op::ISZERO | op::MLOAD | op::SLOAD | op::CALLDATALOAD => (1, 1),
            op::CALLER | op::CALLDATASIZE => (0, 1),
            op::POP | op::JUMP => (1, 0),
            op::MSTORE | op::SSTORE | op::JUMPI | op::RETURN | op::REVERT => (2, 0),
            op::CODECOPY => (3, 0),
            op::DUP1 => (1, 2),
            _ => (0, 0),
        };
        self.height = self.height.saturating_sub(pops) + pushes;
    }

    fn push(&mut self, bytes: Vec<u8>) {
        self.gas += 3;
        self.items.push(Item::Push(bytes));
        self.height += 1;
    }

    fn push_int(&mut self, n: i64) {
        self.push(int_word(n));
    }

    fn push_usize(&mut self, n: usize) {
        let bytes: Vec<u8> = n
            .to_be_bytes()
            .iter()
            .copied()
            .skip_while(|b| *b == 0)
            .collect();
        self.push(if bytes.is_empty() { vec![0] } else { bytes });
    }

    fn label(&mut self) -> usize {
        self.labels += 1;
        self.labels
    }

    fn place(&mut self, l: usize) {
        self.gas += 1;
        self.items.push(Item::Label(l));
    }

    fn push_label(&mut self, l: usize) {
        self.gas += 3;
        self.items.push(Item::PushLabel(l));
        self.height += 1;
    }

    fn jump(&mut self, l: usize) {
        self.push_label(l);
        self.op(op::JUMP);
    }

    fn jumpi(&mut self, l: usize) {
        self.push_label(l);
        self.op(op::JUMPI);
    }

    fn mem(slot: usize) -> usize {
        0x80 + 32 * slot
    }

    fn mload(&mut self, slot: usize) {
        self.push_usize(Self::mem(slot));
        self.op(op::MLOAD);
    }

    fn mstore(&mut self, slot: usize) {
        // value on the stack; MSTORE(offset, value) pops the offset first.
        self.push_usize(Self::mem(slot));
        self.op(op::MSTORE);
    }

    fn fail(&mut self, code: &'static str) -> usize {
        if let Some(l) = self.fail_labels.get(code) {
            return *l;
        }
        let l = self.label();
        self.fail_labels.insert(code, l);
        l
    }

    /// Revert with `code`'s five bytes when the top of the stack is not a sign-extended `i64`;
    /// the value stays.
    fn check_i64(&mut self, code: &'static str) {
        self.op(op::DUP1);
        self.op(op::DUP1);
        self.push_int(7);
        self.op(op::SIGNEXTEND);
        self.op(op::EQ);
        self.op(op::ISZERO);
        let l = self.fail(code);
        self.jumpi(l);
    }

    fn pop_to(&mut self, floor: usize) {
        while self.height > floor {
            self.op(op::POP);
        }
    }

    fn block(&mut self, fr: &mut Frame<'a>, b: &'a Block, result: bool) -> R<bool> {
        for s in &b.stmts {
            self.stmt(fr, s)?;
        }
        match &b.tail {
            Some(t) if result => {
                self.result(fr, t)?;
                Ok(true)
            }
            Some(t) => {
                self.expr(fr, t)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn stmt(&mut self, fr: &mut Frame<'a>, s: &'a Stmt) -> R<()> {
        let f = fr.f;
        match s {
            Stmt::Let { local, value } | Stmt::Assign { local, value, .. } => {
                if Some(*local) == fr.state.map(|(l, _)| l) {
                    return Err(refuse(f, value.at, "it rebinds the state"));
                }
                self.expr(fr, value)?;
                self.mstore(fr.base + local.index());
            }
            Stmt::Eval(e) => {
                self.expr(fr, e)?;
                self.op(op::POP);
            }
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.expr(fr, cond)?;
                self.op(op::ISZERO);
                let other = self.label();
                let end = self.label();
                self.jumpi(other);
                let (g0, h0) = (self.gas, self.height);
                self.block(fr, then, false)?;
                self.pop_to(h0);
                self.jump(end);
                let g_then = self.gas - g0;
                self.gas = g0;
                self.height = h0;
                self.place(other);
                if let Some(o) = otherwise {
                    self.block(fr, o, false)?;
                    self.pop_to(h0);
                }
                let g_else = self.gas - g0;
                self.gas = g0 + g_then.max(g_else);
                self.height = h0;
                self.place(end);
            }
            Stmt::Loop { id, cond, body, .. } => {
                // A loop's turns are not counted: the bound is open.
                self.bounded = false;
                let top = self.label();
                let cont = self.label();
                let end = self.label();
                let h0 = self.height;
                fr.loops.insert(*id, (cont, end, h0));
                self.place(top);
                self.expr(fr, cond)?;
                self.op(op::ISZERO);
                self.jumpi(end);
                self.block(fr, body, false)?;
                self.pop_to(h0);
                self.place(cont);
                self.jump(top);
                self.height = h0;
                self.place(end);
            }
            Stmt::Break { target, at } | Stmt::Continue { target, at } => {
                let Some(&(cont, end, h)) = fr.loops.get(target) else {
                    return Err(refuse(f, *at, "a `break` or `continue` outside its loop"));
                };
                let saved = self.height;
                self.pop_to(h);
                self.jump(if matches!(s, Stmt::Break { .. }) {
                    end
                } else {
                    cont
                });
                self.height = saved;
            }
            Stmt::Return { value, .. } => match fr.ret {
                None => self.result(fr, value)?,
                Some((slot, label)) => {
                    self.expr(fr, value)?;
                    self.mstore(slot);
                    let saved = self.height;
                    self.pop_to(fr.floor);
                    self.jump(label);
                    self.height = saved;
                }
            },
            Stmt::Block(b) => {
                let h0 = self.height;
                self.block(fr, b, false)?;
                self.pop_to(h0);
            }
            Stmt::Match(m) => return Err(refuse(f, m.at, "it matches on an enum")),
            Stmt::Scope { at, .. } | Stmt::Spawn { at, .. } => {
                return Err(refuse(f, *at, "it starts tasks"))
            }
        }
        Ok(())
    }

    /// A field of the state parameter, read through its view.
    fn state_field(&mut self, view: View, field: u32) {
        match view {
            View::Storage => {
                let name = self.fields.get(field as usize).cloned().unwrap_or_default();
                self.push(slot_of(&name).to_vec());
                self.op(op::SLOAD);
            }
            View::Memory(base) => self.mload(base + field as usize),
        }
    }

    /// One value, pushed.
    fn expr(&mut self, fr: &mut Frame<'a>, e: &'a Expr) -> R<()> {
        let f = fr.f;
        // The EVM backend computes in `Int` and `Bool` only (Gate 2 added other numbers to the
        // language, not to this target).
        let other_number =
            e.ty.is_some_and(|t| matches!(t, nazm_core::sema::Type::Num(_)))
                || matches!(&e.kind, ExprKind::Compare { operand, .. }
                if matches!(operand, nazm_core::sema::Type::Num(_)));
        if other_number
            || matches!(
                e.kind,
                ExprKind::Bits { .. }
                    | ExprKind::Complement(_)
                    | ExprKind::Convert { .. }
                    | ExprKind::Fits { .. }
            )
        {
            return Err(refuse(
                f,
                e.at,
                "it uses a number other than `Int`, or a bit operation",
            ));
        }
        match &e.kind {
            ExprKind::Const(k) => match f.constant(*k) {
                Some(Const::Int(n)) => self.push_int(*n),
                Some(Const::Bool(b)) => self.push_int(i64::from(*b)),
                _ => return Err(refuse(f, e.at, "it uses a `Str`")),
            },
            ExprKind::Bits { .. }
            | ExprKind::Complement(_)
            | ExprKind::Convert { .. }
            | ExprKind::Fits { .. } => {
                return Err(refuse(f, e.at, "it uses a bit operation or a conversion"))
            }
            ExprKind::Array { .. } | ExprKind::ArrayRepeat { .. } | ExprKind::Index { .. } => {
                return Err(refuse(f, e.at, "it uses an array"))
            }
            ExprKind::Local(l) => {
                if Some(*l) == fr.caller {
                    self.op(op::CALLER);
                    // A sender address that is not an `Int` cannot be the language's `caller`.
                    self.check_i64("N0397");
                } else if Some(*l) == fr.state.map(|(s, _)| s) {
                    return Err(refuse(
                        f,
                        e.at,
                        "it uses the state other than through `s.field`",
                    ));
                } else {
                    self.mload(fr.base + l.index());
                }
            }
            ExprKind::Field { base, field, .. } => match (&base.kind, fr.state) {
                (ExprKind::Local(l), Some((s, view))) if *l == s => self.state_field(view, *field),
                _ => {
                    return Err(refuse(
                        f,
                        e.at,
                        "it reads a field of a record other than the state",
                    ))
                }
            },
            ExprKind::Neg(x) => {
                self.push_int(0);
                self.expr(fr, x)?;
                // 0 - x: SUB pops the minuend first, so swap the order by pushing x after 0 and
                // subtracting with the operands as SUB(a=top-1? ) — SUB computes s[0] - s[1].
                self.op(op::SWAP1);
                self.op(op::SUB);
                self.check_i64("N0400");
            }
            ExprKind::Arith { op: aop, lhs, rhs } => {
                self.expr(fr, lhs)?;
                self.expr(fr, rhs)?;
                match aop {
                    ArithOp::Add => {
                        self.op(op::ADD);
                        self.check_i64("N0400");
                    }
                    ArithOp::Mul => {
                        self.op(op::MUL);
                        self.check_i64("N0400");
                    }
                    ArithOp::Sub => {
                        // stack: a b → SUB computes top - next = b - a; swap first.
                        self.op(op::SWAP1);
                        self.op(op::SUB);
                        self.check_i64("N0400");
                    }
                    ArithOp::Div | ArithOp::Rem => {
                        // stack: a b. Division by zero first, then i64::MIN / -1.
                        self.op(op::DUP1);
                        self.op(op::ISZERO);
                        let z = self.fail("N0401");
                        self.jumpi(z);
                        if *aop == ArithOp::Div {
                            // b == -1 && a == MIN → overflow.
                            self.op(op::DUP1);
                            self.push_int(-1);
                            self.op(op::EQ);
                            self.op(op::DUP3);
                            self.push_int(i64::MIN);
                            self.op(op::EQ);
                            self.op(op::AND);
                            let o = self.fail("N0400");
                            self.jumpi(o);
                            self.op(op::SWAP1);
                            self.op(op::SDIV);
                        } else {
                            // `SMOD` gives 0 for i64::MIN % -1, as the language does.
                            self.op(op::SWAP1);
                            self.op(op::SMOD);
                        }
                    }
                }
            }
            ExprKind::Compare {
                op: cop, lhs, rhs, ..
            } => {
                self.expr(fr, lhs)?;
                self.expr(fr, rhs)?;
                // stack: a b, b on top. SLT computes top < next, i.e. b < a.
                match cop {
                    CmpOp::Eq => self.op(op::EQ),
                    CmpOp::NotEq => {
                        self.op(op::EQ);
                        self.op(op::ISZERO);
                    }
                    CmpOp::Lt => self.op(op::SGT), // b > a ⇔ a < b
                    CmpOp::Gt => self.op(op::SLT), // b < a ⇔ a > b
                    CmpOp::LtEq => {
                        self.op(op::SLT); // b < a ⇔ a > b
                        self.op(op::ISZERO);
                    }
                    CmpOp::GtEq => {
                        self.op(op::SGT); // b > a ⇔ a < b
                        self.op(op::ISZERO);
                    }
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(fr, cond)?;
                self.op(op::ISZERO);
                let other = self.label();
                let end = self.label();
                self.jumpi(other);
                let (g0, h0) = (self.gas, self.height);
                self.block(fr, then, false)?;
                self.jump(end);
                let g_then = self.gas - g0;
                self.gas = g0;
                self.height = h0;
                self.place(other);
                self.block(fr, otherwise, false)?;
                let g_else = self.gas - g0;
                self.gas = g0 + g_then.max(g_else);
                self.height = h0 + 1;
                self.place(end);
            }
            ExprKind::Block(b) => {
                let h0 = self.height;
                if !self.block(fr, b, false)? {
                    return Err(refuse(f, e.at, "a block with no value"));
                }
                let _ = h0;
            }
            ExprKind::Call { callee, args, .. } => self.inline(fr, *callee, args, e.at)?,
            ExprKind::Builtin { op: b, .. } => {
                return Err(refuse(
                    f,
                    e.at,
                    &format!(
                        "it calls the built-in `{}`, which has no EVM form",
                        b.name()
                    ),
                ))
            }
            ExprKind::Record { .. } | ExprKind::Variant { .. } | ExprKind::Match(_) => {
                return Err(refuse(
                    f,
                    e.at,
                    "it builds a record or an enum other than the result",
                ))
            }
            ExprKind::Closure { .. } | ExprKind::FnValue(_) | ExprKind::CallIndirect { .. } => {
                return Err(refuse(f, e.at, "it uses a function value"))
            }
            ExprKind::CallTrait { .. } => {
                return Err(refuse(
                    f,
                    e.at,
                    "it calls a method through a type parameter's bound",
                ))
            }
        }
        Ok(())
    }

    /// A call to a pure helper, inlined into a frame of its own: its parameters and locals in
    /// fresh memory slots, its `return` a jump to its end with the value in its return slot.
    fn inline(&mut self, fr: &mut Frame<'a>, callee: FnId, args: &'a [Expr], at: Span) -> R<()> {
        let f = fr.f;
        let Some(g) = self.program.function(callee) else {
            return Err(refuse(f, at, "a callee with no body"));
        };
        self.depth += 1;
        if self.depth > 32 {
            return Err(refuse(f, at, "calls nested deeper than 32"));
        }
        if !g.effects.is_pure() || g.foreign.is_some() || g.closure {
            return Err(refuse(
                g,
                g.name_span,
                "a callee that is not a pure Nazm function",
            ));
        }
        let base = self.next_slot;
        self.next_slot += g.locals.len() + 1;
        let ret_slot = self.next_slot - 1;
        // The arguments, evaluated in order, then stored last-first.
        let mut state = None;
        let mut stored = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if g.local(LocalId(u32::try_from(i).unwrap_or(u32::MAX)))
                .map(|l| l.ty)
                == Some(self.state_ty)
            {
                // The state passed to a helper, as the frame's own state parameter.
                match (&a.kind, fr.state) {
                    (ExprKind::Local(l), Some((s, view))) if *l == s => {
                        state = Some((LocalId(u32::try_from(i).unwrap_or(u32::MAX)), view));
                        continue;
                    }
                    _ => return Err(refuse(f, a.at, "it passes a state other than its own")),
                }
            }
            self.expr(fr, a)?;
            stored.push(i);
        }
        for i in stored.into_iter().rev() {
            self.mstore(base + i);
        }
        let end = self.label();
        let floor = self.height;
        let mut inner = Frame {
            f: g,
            base,
            ret: Some((ret_slot, end)),
            floor,
            state,
            caller: None,
            loops: BTreeMap::new(),
        };
        if self.block(&mut inner, &g.body, false)? {
            self.mstore(ret_slot);
        }
        self.pop_to(floor);
        self.place(end);
        self.mload(ret_slot);
        self.depth -= 1;
        Ok(())
    }

    /// What an entrypoint returns: commit the new state (after the conservation check) and stop,
    /// or revert with an error code.
    fn result(&mut self, fr: &mut Frame<'a>, e: &'a Expr) -> R<()> {
        let f = fr.f;
        match &e.kind {
            ExprKind::Variant {
                variant: 1, fields, ..
            } => {
                let Some(init) = fields.first() else {
                    return Err(refuse(f, e.at, "an `Err` without its code"));
                };
                self.expr(fr, &init.value)?;
                self.push_int(0);
                self.op(op::MSTORE);
                self.push_int(32);
                self.push_int(0);
                self.op(op::REVERT);
            }
            ExprKind::Variant {
                variant: 0, fields, ..
            } => {
                let Some(init) = fields.first() else {
                    return Err(refuse(f, e.at, "an `Ok` without its state"));
                };
                match &init.value.kind {
                    ExprKind::Local(l) if Some(*l) == fr.state.map(|(s, _)| s) => self.op(op::STOP),
                    ExprKind::Record { fields: inits, .. } => self.commit(fr, inits, e.at)?,
                    _ => {
                        return Err(refuse(
                            f,
                            init.value.at,
                            "it returns a state built somewhere other than in its result",
                        ))
                    }
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(fr, cond)?;
                self.op(op::ISZERO);
                let other = self.label();
                self.jumpi(other);
                let (g0, h0) = (self.gas, self.height);
                if !self.block(fr, then, true)? {
                    self.op(op::STOP);
                }
                let g_then = self.gas - g0;
                self.gas = g0;
                self.height = h0;
                self.place(other);
                if !self.block(fr, otherwise, true)? {
                    self.op(op::STOP);
                }
                let g_else = self.gas - g0;
                self.gas = g0 + g_then.max(g_else);
            }
            ExprKind::Block(b) => {
                if !self.block(fr, b, true)? {
                    self.op(op::STOP);
                }
            }
            _ => return Err(refuse(f, e.at, "a result that is not `Ok(…)` or `Err(…)`")),
        }
        Ok(())
    }

    /// The new state: every field's new value into memory, the conserved quantity compared before
    /// and after, then each written field stored.
    fn commit(
        &mut self,
        fr: &mut Frame<'a>,
        inits: &'a [nazm_core::cir::ir::Init],
        at: Span,
    ) -> R<()> {
        let n = self.fields.len();
        let base = self.next_slot;
        self.next_slot += n;
        let mut written = vec![false; n];
        for k in 0..n {
            let field = u32::try_from(k).unwrap_or(u32::MAX);
            match inits.iter().find(|i| i.field == field) {
                Some(init) => {
                    let passthrough = matches!(
                        &init.value.kind,
                        ExprKind::Field { base: b, field: g, .. }
                            if *g == field && matches!(b.kind, ExprKind::Local(l) if Some(l) == fr.state.map(|(s, _)| s))
                    );
                    self.expr(fr, &init.value)?;
                    if let Some(w) = written.get_mut(k) {
                        *w = !passthrough;
                    }
                }
                None => return Err(refuse(fr.f, at, "a state with a field missing")),
            }
            self.mstore(base + k);
        }
        if let Some(c) = self.conserved {
            let Some(g) = self.program.function(c) else {
                return Err(refuse(fr.f, at, "`conserved` has no body"));
            };
            let before = Frame {
                f: g,
                base: 0,
                ret: None,
                floor: 0,
                state: Some((LocalId(0), View::Storage)),
                caller: None,
                loops: BTreeMap::new(),
            };
            let after = Frame {
                state: Some((LocalId(0), View::Memory(base))),
                ..before.clone()
            };
            for mut view in [before, after] {
                let frame_base = self.next_slot;
                self.next_slot += g.locals.len() + 1;
                let ret_slot = self.next_slot - 1;
                let end = self.label();
                view.base = frame_base;
                view.ret = Some((ret_slot, end));
                view.floor = self.height;
                if self.block(&mut view, &g.body, false)? {
                    self.mstore(ret_slot);
                }
                self.pop_to(view.floor);
                self.place(end);
                self.mload(ret_slot);
            }
            self.op(op::EQ);
            self.op(op::ISZERO);
            let l = self.fail("N0396");
            self.jumpi(l);
        }
        for (k, name) in self.fields.to_vec().iter().enumerate() {
            if written.get(k).copied().unwrap_or(false) {
                self.mload(base + k);
                self.push(slot_of(name).to_vec());
                self.op(op::SSTORE);
            }
        }
        self.op(op::STOP);
        Ok(())
    }
}

/// Resolve labels: every label push is `PUSH2`, so offsets are known after one pass.
fn assemble(items: &[Item]) -> Vec<u8> {
    let mut at = BTreeMap::new();
    let mut pc = 0usize;
    for i in items {
        match i {
            Item::Op(_) => pc += 1,
            Item::Push(b) => pc += 1 + b.len(),
            Item::Label(l) => {
                at.insert(*l, pc);
                pc += 1;
            }
            Item::PushLabel(_) => pc += 3,
        }
    }
    let mut out = Vec::with_capacity(pc);
    for i in items {
        match i {
            Item::Op(o) => out.push(*o),
            Item::Push(b) => {
                out.push(0x5f + u8::try_from(b.len()).unwrap_or(32));
                out.extend_from_slice(b);
            }
            Item::Label(_) => out.push(op::JUMPDEST),
            Item::PushLabel(l) => {
                let to = at.get(l).copied().unwrap_or(0);
                out.push(0x61);
                out.extend_from_slice(&u16::try_from(to).unwrap_or(u16::MAX).to_be_bytes());
            }
        }
    }
    out
}

/// The failure stubs: revert with the code's five bytes.
fn stubs(g: &mut Gen<'_>) {
    for (code, l) in g.fail_labels.clone() {
        g.place(l);
        let mut word = vec![0u8; 27];
        word.extend_from_slice(code.as_bytes());
        g.push(word.iter().copied().skip_while(|b| *b == 0).collect());
        g.push_int(0);
        g.op(op::MSTORE);
        g.push_int(5);
        g.push_int(27);
        g.op(op::REVERT);
    }
}

/// The contract, compiled.
///
/// # Errors
///
/// The first construct the backend does not emit, by function, span and reason.
pub fn compile(
    program: &Program,
    res: &Resolution,
    root: Option<ModuleId>,
    c: &Contract,
) -> Result<Evm, Refusal> {
    let find = |name: &str| {
        program
            .functions
            .iter()
            .find(|f| f.name == name && Some(f.module) == root)
    };
    let state_ty = Type::Record(c.state);
    let types = res.types();
    let storage: Vec<Slot> = res
        .records()
        .get(c.state)
        .map(|d| {
            d.fields
                .iter()
                .map(|f| Slot {
                    field: f.name.clone(),
                    ty: if f.ty == Type::Bool { "Bool" } else { "Int" },
                    slot: slot_of(&f.name),
                })
                .collect()
        })
        .unwrap_or_default();
    let _ = types;
    let mut g = Gen {
        program,
        fields: &c.fields,
        state_ty,
        items: Vec::new(),
        labels: 0,
        next_slot: 0,
        height: 0,
        gas: 0,
        bounded: true,
        depth: 0,
        fail_labels: BTreeMap::new(),
        conserved: c
            .conserved
            .then(|| find("conserved").map(|f| f.def))
            .flatten(),
    };
    // Dispatch on the selector.
    let mut abi = Vec::new();
    let mut entries = Vec::new();
    for e in &c.entrypoints {
        let signature = format!("{}({})", e.name, vec!["int64"; e.args.len()].join(","));
        let h = keccak256(signature.as_bytes());
        let selector = [h[0], h[1], h[2], h[3]];
        let l = g.label();
        entries.push((e, l));
        abi.push(Abi {
            name: e.name.clone(),
            signature,
            selector,
            args: e.args.clone(),
            gas_bound: None,
        });
    }
    g.push_int(4);
    g.op(op::CALLDATASIZE);
    g.op(op::LT);
    let short = g.fail("N0398");
    g.jumpi(short);
    g.push_int(0);
    g.op(op::CALLDATALOAD);
    g.push_int(224);
    g.op(op::SHR);
    for (a, (_, l)) in abi.iter().zip(entries.iter()) {
        g.op(op::DUP1);
        g.push(a.selector.to_vec());
        g.op(op::EQ);
        g.jumpi(*l);
    }
    let unknown = g.fail("N0398");
    g.jump(unknown);
    for (i, (e, l)) in entries.iter().enumerate() {
        let Some(f) = find(&e.name) else { continue };
        g.place(*l);
        g.height = 1;
        g.op(op::POP);
        g.gas = 0;
        g.bounded = true;
        g.next_slot = 0;
        let base = g.next_slot;
        g.next_slot += f.locals.len();
        // The arguments: each a 32-byte word that must be a sign-extended `i64`.
        g.push_int(i64::try_from(4 + 32 * e.args.len()).unwrap_or(i64::MAX));
        g.op(op::CALLDATASIZE);
        g.op(op::LT);
        let bad = g.fail("N0398");
        g.jumpi(bad);
        for k in 0..e.args.len() {
            g.push_int(i64::try_from(4 + 32 * k).unwrap_or(0));
            g.op(op::CALLDATALOAD);
            g.check_i64("N0398");
            g.mstore(base + 2 + k);
        }
        let mut fr = Frame {
            f,
            base,
            ret: None,
            floor: 0,
            state: Some((LocalId(0), View::Storage)),
            caller: Some(LocalId(1)),
            loops: BTreeMap::new(),
        };
        if !g.block(&mut fr, &f.body, true)? {
            g.op(op::STOP);
        }
        // Memory: the slots this entrypoint used, at 3 gas a word plus the quadratic term; and
        // the dispatch that reached it: the length check and every selector compared before it.
        let words = u64::try_from(4 + g.next_slot).unwrap_or(u64::MAX);
        let memory = 3 * words + ((words * words) >> 9);
        let dispatch = 40 + 22 * u64::try_from(i + 1).unwrap_or(u64::MAX);
        if let Some(a) = abi.get_mut(i) {
            a.gas_bound = g.bounded.then_some(g.gas + memory + dispatch);
        }
    }
    stubs(&mut g);
    let runtime = assemble(&g.items);

    // Deployment: `init()`'s state into storage, then the runtime returned.
    let Some(init) = find("init") else {
        return Err(Refusal {
            function: "init".to_owned(),
            at: Span::synthetic(),
            why: "no `init`".to_owned(),
        });
    };
    let mut d = Gen {
        program,
        fields: &c.fields,
        state_ty,
        items: Vec::new(),
        labels: 0,
        next_slot: 0,
        height: 0,
        gas: 0,
        bounded: true,
        depth: 0,
        fail_labels: BTreeMap::new(),
        conserved: None,
    };
    let mut fr = Frame {
        f: init,
        base: 0,
        ret: None,
        floor: 0,
        state: None,
        caller: None,
        loops: BTreeMap::new(),
    };
    d.next_slot = init.locals.len();
    for s in &init.body.stmts {
        d.stmt(&mut fr, s)?;
    }
    let Some(ExprKind::Record { fields: inits, .. }) = init.body.tail.as_deref().map(|t| &t.kind)
    else {
        return Err(refuse(
            init,
            init.name_span,
            "`init` does not end in `State(…)`",
        ));
    };
    for init_field in inits {
        let name = c
            .fields
            .get(init_field.field as usize)
            .cloned()
            .unwrap_or_default();
        d.expr(&mut fr, &init_field.value)?;
        d.push(slot_of(&name).to_vec());
        d.op(op::SSTORE);
    }
    let code_at = d.label();
    d.push_usize(runtime.len());
    d.op(op::DUP1);
    d.push_label(code_at);
    d.push_int(0);
    d.op(op::CODECOPY);
    d.push_int(0);
    d.op(op::RETURN);
    stubs(&mut d);
    // The runtime follows the deployment code; `code_at` marks where it starts.
    d.items.push(Item::Label(code_at));
    let mut deploy = assemble(&d.items);
    // The label's own `JUMPDEST` byte is the runtime's offset; drop it, so the runtime begins
    // exactly there.
    deploy.pop();
    deploy.extend_from_slice(&runtime);
    Ok(Evm {
        deploy,
        runtime,
        abi,
        storage,
    })
}
