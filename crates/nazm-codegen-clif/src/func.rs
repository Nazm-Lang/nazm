//! One MIR function, as Cranelift IR.
//!
//! Each MIR block is a Cranelift block and each terminator its terminator; a local of a
//! register type is a Cranelift variable, a `Str`, record or enum a stack slot (or, for a
//! parameter, the caller's memory it borrows). A drop, a copy and a move are the calls and stores
//! MIR says they are; a failure edge is a branch to the Cranelift block of the MIR block it
//! names. Nothing here decides anything about the language.

use std::collections::BTreeMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types, Block, FuncRef, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, TrapCode, Value,
};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_module::{FuncId, Module};
use nazm_lir::{EnumTy, RecordTy, Sig, Ty};
use nazm_mir as mir;
use nazm_span::Span;

use crate::layout::{in_memory, PAYLOAD};
use crate::unit::{Gen, Helper};

/// Where a MIR local lives.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Loc {
    Var(Variable, Ty),
    /// In a stack slot, or — a parameter — in memory the caller owns, by its pointer.
    Mem(MemAt, Ty),
    Tasks(Variable),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MemAt {
    Slot(cranelift_codegen::ir::StackSlot),
    Borrowed(Value),
}

pub(crate) struct Fx<'g, 'a, 'b> {
    pub g: &'g mut Gen<'a>,
    pub b: FunctionBuilder<'b>,
    refs: BTreeMap<FuncId, FuncRef>,
    pub locals: Vec<Loc>,
    blocks: Vec<Block>,
    sret: Option<Value>,
    /// The Cranelift block the statement being translated fails to.
    pub unwind: Option<Block>,
    pub at: Span,
    /// What the closure object of the function being translated carries, if it is a
    /// function value's code (N50).
    carried: Vec<Ty>,
}

pub(crate) fn mem() -> MemFlagsData {
    MemFlagsData::trusted()
}

pub(crate) fn translate(
    g: &mut Gen<'_>,
    ctx: &mut Context,
    fctx: &mut FunctionBuilderContext,
    f: &mir::Function,
    sig: &Sig,
) {
    ctx.func.signature = g.signature(&sig.params, sig.ret);
    let b = FunctionBuilder::new(&mut ctx.func, fctx);
    let mut fx = Fx {
        g,
        b,
        refs: BTreeMap::new(),
        locals: Vec::new(),
        blocks: Vec::new(),
        sret: None,
        unwind: None,
        at: f.span,
        carried: f.closure.iter().flatten().map(|t| Ty::of(*t)).collect(),
    };
    let entry = fx.b.create_block();
    fx.b.append_block_params_for_function_params(entry);
    fx.b.switch_to_block(entry);
    let params: Vec<Value> = fx.b.block_params(entry).to_vec();
    let mut next_param = 0usize;
    if in_memory(sig.ret) {
        fx.sret = params.first().copied();
        next_param = 1;
    }
    for (i, l) in f.locals.iter().enumerate() {
        let loc = match l.ty.value().map(Ty::of) {
            None => {
                let v = fx.b.declare_var(types::I64);
                let z = fx.b.ins().iconst(types::I64, 0);
                fx.b.def_var(v, z);
                Loc::Tasks(v)
            }
            Some(t) if in_memory(t) => {
                if i < f.params as usize {
                    let p = params.get(next_param).copied();
                    next_param += 1;
                    match p {
                        Some(p) => Loc::Mem(MemAt::Borrowed(p), t),
                        None => {
                            fx.g.defect(f.span, "a parameter with no argument");
                            Loc::Mem(MemAt::Borrowed(fx.b.ins().iconst(types::I64, 0)), t)
                        }
                    }
                } else {
                    let lay =
                        fx.g.lay
                            .lay(t)
                            .unwrap_or(crate::layout::Lay { size: 8, align: 8 });
                    let slot = fx.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        lay.size.max(1),
                        3,
                    ));
                    if fx.g.lay.owns(t) {
                        let a = fx.b.ins().stack_addr(fx.g.ptr, slot, 0);
                        fx.zero(a, lay.size);
                    }
                    Loc::Mem(MemAt::Slot(slot), t)
                }
            }
            Some(t) => {
                let v = fx.b.declare_var(Gen::clif(t));
                if i < f.params as usize {
                    let p = params.get(next_param).copied();
                    next_param += 1;
                    let p = p.unwrap_or_else(|| fx.b.ins().iconst(Gen::clif(t), 0));
                    fx.b.def_var(v, p);
                } else {
                    let z = fx.b.ins().iconst(Gen::clif(t), 0);
                    fx.b.def_var(v, z);
                }
                Loc::Var(v, t)
            }
        };
        fx.locals.push(loc);
    }
    for _ in &f.blocks {
        let blk = fx.b.create_block();
        fx.blocks.push(blk);
    }
    if let Some(first) = fx.blocks.first().copied() {
        fx.b.ins().jump(first, &[]);
    }
    for (i, blk) in f.blocks.iter().enumerate() {
        let Some(cb) = fx.blocks.get(i).copied() else {
            continue;
        };
        fx.b.switch_to_block(cb);
        for s in &blk.stmts {
            fx.at = s.at;
            fx.unwind = s
                .kind
                .unwind()
                .and_then(|u| fx.blocks.get(u.index()).copied());
            fx.statement(&s.kind);
        }
        fx.at = blk.term_at;
        fx.terminator(&blk.term, sig.ret);
    }
    fx.b.seal_all_blocks();
    let cfg = fx.g.module.target_config();
    fx.b.finalize(cfg);
}

impl<'g, 'a, 'b> Fx<'g, 'a, 'b> {
    /// A context for a generated helper: no MIR locals, blocks or failure edge.
    pub(crate) fn helper(g: &'g mut Gen<'a>, b: FunctionBuilder<'b>) -> Self {
        Self {
            g,
            b,
            refs: BTreeMap::new(),
            locals: Vec::new(),
            blocks: Vec::new(),
            sret: None,
            unwind: None,
            at: Span::synthetic(),
            carried: Vec::new(),
        }
    }

    pub(crate) fn func(&mut self, id: FuncId) -> FuncRef {
        if let Some(r) = self.refs.get(&id) {
            return *r;
        }
        let r = self.g.module.declare_func_in_func(id, self.b.func);
        self.refs.insert(id, r);
        r
    }

    /// Call a runtime entry point from the registry.
    pub(crate) fn rt(&mut self, symbol: &'static str, args: &[Value]) -> Option<Value> {
        let id = self.g.import(symbol);
        let r = self.func(id);
        let inst = self.b.ins().call(r, args);
        self.b.inst_results(inst).first().copied()
    }

    pub(crate) fn iconst(&mut self, v: i64) -> Value {
        self.b.ins().iconst(types::I64, v)
    }

    pub(crate) fn zero(&mut self, addr: Value, size: u32) {
        let cfg = self.g.module.target_config();
        self.b
            .emit_small_memset(cfg, addr, 0, u64::from(size), 1, mem());
    }

    pub(crate) fn copy(&mut self, dst: Value, src: Value, size: u32) {
        let cfg = self.g.module.target_config();
        self.b
            .emit_small_memory_copy(cfg, dst, src, u64::from(size), 1, 1, true, mem());
    }

    pub(crate) fn size(&self, t: Ty) -> u32 {
        self.g.lay.lay(t).map_or(8, |l| l.size)
    }

    pub(crate) fn ty(&self, l: mir::LocalId) -> Ty {
        match self.locals.get(l.index()) {
            Some(Loc::Var(_, t) | Loc::Mem(_, t)) => *t,
            _ => Ty::Int,
        }
    }

    /// The address of an in-memory local.
    pub(crate) fn addr(&mut self, l: mir::LocalId) -> Value {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Mem(MemAt::Slot(s), _)) => self.b.ins().stack_addr(self.g.ptr, s, 0),
            Some(Loc::Mem(MemAt::Borrowed(p), _)) => p,
            _ => {
                self.g
                    .defect(self.at, "an address of a local held in a register");
                self.iconst(0)
            }
        }
    }

    /// The value of a register local.
    pub(crate) fn get(&mut self, l: mir::LocalId) -> Value {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Var(v, _) | Loc::Tasks(v)) => self.b.use_var(v),
            _ => {
                self.g
                    .defect(self.at, "a register read of a local held in memory");
                self.iconst(0)
            }
        }
    }

    pub(crate) fn set(&mut self, l: mir::LocalId, v: Value) {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Var(var, _) | Loc::Tasks(var)) => self.b.def_var(var, v),
            _ => self
                .g
                .defect(self.at, "a register write of a local held in memory"),
        }
    }

    /// An operand as a register value.
    pub(crate) fn value(&mut self, o: &mir::Operand) -> Value {
        match o {
            mir::Operand::Local(l) => self.get(*l),
            mir::Operand::Const(mir::Const::Int(v)) => self.iconst(*v),
            mir::Operand::Const(mir::Const::Bool(v)) => {
                self.b.ins().iconst(types::I8, i64::from(*v))
            }
            mir::Operand::Const(mir::Const::Str(_)) => {
                self.g
                    .defect(self.at, "a string literal read into a register");
                self.iconst(0)
            }
        }
    }

    /// An operand as the address of its value.
    pub(crate) fn place(&mut self, o: &mir::Operand) -> Value {
        match o {
            mir::Operand::Local(l) => self.addr(*l),
            mir::Operand::Const(mir::Const::Str(bytes)) => {
                let d = self.g.str_value(bytes);
                let gv = self.g.module.declare_data_in_func(d, self.b.func);
                self.b.ins().symbol_value(self.g.ptr, gv)
            }
            mir::Operand::Const(_) => {
                self.g.defect(self.at, "the address of a scalar constant");
                self.iconst(0)
            }
        }
    }

    pub(crate) fn operand_ty(&self, o: &mir::Operand) -> Ty {
        match o {
            mir::Operand::Local(l) => self.ty(*l),
            mir::Operand::Const(c) => Ty::of(c.ty()),
        }
    }

    /// An argument to a Nazm function: its address if it lives in memory, its value otherwise.
    pub(crate) fn argument(&mut self, o: &mir::Operand) -> Value {
        if in_memory(self.operand_ty(o)) {
            self.place(o)
        } else {
            self.value(o)
        }
    }

    // --- failure -------------------------------------------------------------------------

    /// Record the failure and leave along the statement's failure edge.
    pub(crate) fn fail(&mut self, code: &str, message: &str) {
        let (d, n) = self.g.message(code, message, self.at);
        let gv = self.g.module.declare_data_in_func(d, self.b.func);
        let p = self.b.ins().symbol_value(self.g.ptr, gv);
        let len = self.iconst(i64::try_from(n).unwrap_or(0));
        self.rt("nz.fail", &[p, len]);
        self.leave();
    }

    /// Jump along the failure edge.
    pub(crate) fn leave(&mut self) {
        match self.unwind {
            Some(u) => {
                self.b.ins().jump(u, &[]);
            }
            None => {
                self.g
                    .defect(self.at, "a failure with no failure edge in MIR");
                self.b.ins().trap(TrapCode::unwrap_user(1));
            }
        }
    }

    /// Fail with this diagnostic when `bad` is true; continue in a fresh block otherwise.
    pub(crate) fn guard(&mut self, bad: Value, code: &str, message: &str) {
        let failing = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(bad, failing, &[], cont, &[]);
        self.b.switch_to_block(failing);
        self.fail(code, message);
        self.b.switch_to_block(cont);
    }

    /// Leave along the failure edge if this thread has failed.
    pub(crate) fn check_failed(&mut self) {
        let f = self
            .rt("nz.failed_now", &[])
            .unwrap_or_else(|| self.iconst(0));
        let cont = self.b.create_block();
        match self.unwind {
            Some(u) => {
                self.b.ins().brif(f, u, &[], cont, &[]);
            }
            None => {
                self.g
                    .defect(self.at, "a failure check with no failure edge");
                self.b.ins().jump(cont, &[]);
            }
        }
        self.b.switch_to_block(cont);
    }

    // --- ownership -----------------------------------------------------------------------

    /// Take a reference to what the value of type `t` at `addr` owns.
    pub(crate) fn retain_at(&mut self, addr: Value, t: Ty) {
        if !self.g.lay.owns(t) {
            return;
        }
        match t {
            Ty::Str => {
                let owner = self.b.ins().load(types::I64, mem(), addr, 16);
                self.rt("nz.str_retain", &[owner]);
            }
            Ty::Record(_) | Ty::Enum(_) => {
                let h = self.g.helper(Helper::Retain(t));
                let r = self.func(h);
                self.b.ins().call(r, &[addr]);
            }
            _ => {
                let v = self.b.ins().load(types::I64, mem(), addr, 0);
                self.retain_value(v, t);
            }
        }
    }

    /// Take a reference to what a handle owns.
    pub(crate) fn retain_value(&mut self, v: Value, t: Ty) {
        match t {
            Ty::Ints | Ty::Strs | Ty::Vec(_) => {
                self.rt("nz.arr_retain", &[v]);
            }
            // A generic channel's count is where an `Int` channel's is (N53).
            Ty::Chan | Ty::ChanOf(_) => {
                self.rt("nz.chan_retain", &[v]);
            }
            Ty::Fn(_) => {
                self.g.needs.closures = true;
                self.rt("nz.closure_retain", &[v]);
            }
            _ => {}
        }
    }

    /// Give back what the value of type `t` at `addr` owns.
    pub(crate) fn release_at(&mut self, addr: Value, t: Ty) {
        if !self.g.lay.owns(t) {
            return;
        }
        match t {
            Ty::Str => {
                let owner = self.b.ins().load(types::I64, mem(), addr, 16);
                self.rt("nz.str_release", &[owner]);
            }
            Ty::Record(_) | Ty::Enum(_) => {
                let h = self.g.helper(Helper::Release(t));
                let r = self.func(h);
                self.b.ins().call(r, &[addr]);
            }
            _ => {
                let v = self.b.ins().load(types::I64, mem(), addr, 0);
                self.release_value(v, t);
            }
        }
    }

    /// Give back what a handle owns.
    pub(crate) fn release_value(&mut self, v: Value, t: Ty) {
        match t {
            Ty::Ints => {
                self.rt("nz.seq_release", &[v]);
            }
            Ty::Strs => {
                self.g.needs.strings = true;
                self.rt("nz.strs_release", &[v]);
            }
            Ty::Chan => {
                self.rt("nz.chan_release", &[v]);
            }
            Ty::ChanOf(_) => {
                self.rt("nz.chanv_release", &[v]);
            }
            Ty::Fn(_) => {
                self.g.needs.closures = true;
                self.rt("nz.closure_release", &[v]);
            }
            Ty::Vec(id) => {
                let elem = self
                    .g
                    .unit
                    .vecs
                    .iter()
                    .find(|l| l.id == id)
                    .map_or(Ty::Int, |l| l.elem);
                if self.g.lay.owns(elem) {
                    let h = self.g.helper(Helper::ReleaseElement(elem));
                    let r = self.func(h);
                    let f = self.b.ins().func_addr(self.g.ptr, r);
                    let stride = self.iconst(i64::from(self.size(elem)));
                    self.rt("nz.vec_release", &[v, f, stride]);
                } else {
                    self.rt("nz.seq_release", &[v]);
                }
            }
            _ => {}
        }
    }

    /// Move the value of local `from` into memory at `to`, emptying `from`.
    pub(crate) fn move_into(&mut self, to: Value, from: mir::LocalId) {
        let t = self.ty(from);
        if in_memory(t) {
            let src = self.addr(from);
            let n = self.size(t);
            self.copy(to, src, n);
            if self.g.lay.owns(t) {
                self.zero(src, n);
            }
        } else {
            let v = self.get(from);
            self.b.ins().store(mem(), v, to, 0);
            if self.g.lay.owns(t) {
                let z = self.iconst(0);
                self.set(from, z);
            }
        }
    }

    /// Where each capture sits in a closure object, after its 24-byte header, and the
    /// object's size (N50). Each in an 8-aligned slot of its own size.
    pub(crate) fn closure_layout(&self, carried: &[Ty]) -> (Vec<u32>, u32) {
        let mut off = 24u32;
        let mut offsets = Vec::with_capacity(carried.len());
        for t in carried {
            offsets.push(off);
            off += self.size(*t).div_ceil(8) * 8;
        }
        (offsets, off)
    }

    /// Read a value of type `t` at `addr` into local `dst`, taking a reference of its own.
    fn read_owned(&mut self, dst: mir::LocalId, addr: Value, t: Ty) {
        if in_memory(t) {
            let d = self.addr(dst);
            let n = self.size(t);
            self.copy(d, addr, n);
            self.retain_at(d, t);
        } else {
            let v = self.b.ins().load(Gen::clif(t), mem(), addr, 0);
            if self.g.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.set(dst, v);
        }
    }

    // --- layout ----------------------------------------------------------------------------

    pub(crate) fn field_offset(&mut self, f: mir::FieldRef) -> (u32, Ty) {
        let rid = RecordTy(u32::try_from(f.record.index()).unwrap_or(u32::MAX));
        let pos = self
            .g
            .unit
            .records
            .iter()
            .find(|r| r.id == rid)
            .and_then(|r| r.of_declaration.get(f.field as usize).copied())
            .unwrap_or(0);
        match self.g.lay.records.get(&rid.0) {
            Some(s) => (
                s.offsets.get(pos as usize).copied().unwrap_or(0),
                s.fields.get(pos as usize).copied().unwrap_or(Ty::Int),
            ),
            None => {
                self.g.defect(self.at, "a record with no layout");
                (0, Ty::Int)
            }
        }
    }

    pub(crate) fn tag(&self, e: nazm_sema::EnumId, variant: u32) -> u32 {
        let eid = EnumTy(u32::try_from(e.index()).unwrap_or(u32::MAX));
        self.g
            .unit
            .enums
            .iter()
            .find(|l| l.id == eid)
            .and_then(|l| l.of_declaration.get(variant as usize).copied())
            .unwrap_or(0)
    }

    pub(crate) fn payload_offset(
        &mut self,
        e: nazm_sema::EnumId,
        tag: u32,
        field: u32,
    ) -> (u32, Ty) {
        let eid = EnumTy(u32::try_from(e.index()).unwrap_or(u32::MAX));
        let pos = self
            .g
            .unit
            .enums
            .iter()
            .find(|l| l.id == eid)
            .and_then(|l| l.variants.get(tag as usize))
            .and_then(|v| v.of_declaration.get(field as usize).copied())
            .unwrap_or(0);
        match self
            .g
            .lay
            .enums
            .get(&eid.0)
            .and_then(|s| s.variants.get(tag as usize))
        {
            Some(v) => (
                PAYLOAD + v.offsets.get(pos as usize).copied().unwrap_or(0),
                v.fields.get(pos as usize).copied().unwrap_or(Ty::Int),
            ),
            None => {
                self.g.defect(self.at, "an enum with no layout");
                (PAYLOAD, Ty::Int)
            }
        }
    }

    // --- statements --------------------------------------------------------------------------

    fn statement(&mut self, s: &mir::StatementKind) {
        match s {
            mir::StatementKind::Assign { dst, value, .. } => self.assign(*dst, value),
            mir::StatementKind::SetField { base, path, value } => {
                let mut a = self.addr(*base);
                let mut last = Ty::Int;
                for step in path {
                    let (off, t) = self.field_offset(*step);
                    a = self.b.ins().iadd_imm_s(a, i64::from(off));
                    last = t;
                }
                // The new value already owns its reference, so the old one may go first.
                self.release_at(a, last);
                self.move_into(a, *value);
            }
            mir::StatementKind::Drop(l) => {
                let t = self.ty(*l);
                if !self.g.lay.owns(t) {
                    return;
                }
                if in_memory(t) {
                    let a = self.addr(*l);
                    self.release_at(a, t);
                    let n = self.size(t);
                    self.zero(a, n);
                } else {
                    let v = self.get(*l);
                    self.release_value(v, t);
                    let z = self.iconst(0);
                    self.set(*l, z);
                }
            }
            mir::StatementKind::ScopeOpen { list, .. } => {
                let t = self.rt("nz.arr_new", &[]).unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, t, 0);
                self.guard(null, "N0406", "out of memory");
                self.set(*list, t);
            }
            mir::StatementKind::Spawn {
                list, callee, args, ..
            } => self.spawn(*list, *callee, args),
            mir::StatementKind::ScopeJoin { list } => {
                let t = self.get(*list);
                let z = self.iconst(0);
                self.set(*list, z);
                let out = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16,
                    3,
                ));
                let o = self.b.ins().stack_addr(self.g.ptr, out, 0);
                self.rt("nz.scope_join_to", &[t, o]);
                let msg = self.b.ins().load(types::I64, mem(), o, 0);
                let has = self.b.ins().icmp_imm_s(IntCC::NotEqual, msg, 0);
                let take = self.b.create_block();
                let cont = self.b.create_block();
                self.b.ins().brif(has, take, &[], cont, &[]);
                self.b.switch_to_block(take);
                let len = self.b.ins().load(types::I64, mem(), o, 8);
                self.rt("nz.fail", &[msg, len]);
                self.b.ins().jump(cont, &[]);
                self.b.switch_to_block(cont);
                // The list of task ids is the runtime's, dead once joined.
                self.rt("nz.arr_release", &[t]);
            }
            mir::StatementKind::CheckFailed { .. } => self.check_failed(),
            mir::StatementKind::StackCheck { .. } => {
                // The runtime measures the thread's stack (N49); this only asks it.
                self.g.needs.stack = true;
                let past = self
                    .rt("nz.stack_check", &[])
                    .unwrap_or_else(|| self.iconst(0));
                self.guard(past, "N0408", nazm_lir::STACK_EXHAUSTED);
            }
        }
    }

    fn assign(&mut self, dst: mir::LocalId, value: &mir::Rvalue) {
        let dt = self.ty(dst);
        match value {
            mir::Rvalue::Use(o) => {
                if in_memory(dt) {
                    let src = self.place(o);
                    let d = self.addr(dst);
                    let n = self.size(dt);
                    self.copy(d, src, n);
                } else {
                    let v = self.value(o);
                    self.set(dst, v);
                }
            }
            mir::Rvalue::Copy(l) => {
                if in_memory(dt) {
                    let src = self.addr(*l);
                    let d = self.addr(dst);
                    let n = self.size(dt);
                    self.copy(d, src, n);
                    self.retain_at(d, dt);
                } else {
                    let v = self.get(*l);
                    self.retain_value(v, dt);
                    self.set(dst, v);
                }
            }
            mir::Rvalue::Move(l) => {
                if in_memory(dt) {
                    let d = self.addr(dst);
                    self.move_into(d, *l);
                } else {
                    let v = self.get(*l);
                    let z = self.iconst(0);
                    self.set(*l, z);
                    self.set(dst, v);
                }
            }
            mir::Rvalue::Neg(o) => {
                let x = self.value(o);
                let z = self.iconst(0);
                let (r, of) = self.b.ins().ssub_overflow(z, x);
                self.guard(of, "N0400", "this negation overflows `Int`");
                self.set(dst, r);
            }
            mir::Rvalue::Arith(op, a, b) => {
                let x = self.value(a);
                let y = self.value(b);
                let r = self.arith(*op, x, y);
                self.set(dst, r);
            }
            mir::Rvalue::Compare {
                op,
                operand,
                lhs,
                rhs,
                ..
            } => {
                let t = Ty::of(*operand);
                let r = match t {
                    Ty::Str => {
                        let a = self.place(lhs);
                        let b = self.place(rhs);
                        let eq = self.string_equal(a, b);
                        if *op == mir::CmpOp::NotEq {
                            self.b.ins().bxor_imm_s(eq, 1)
                        } else {
                            eq
                        }
                    }
                    Ty::Record(_) | Ty::Enum(_) => {
                        let a = self.place(lhs);
                        let b = self.place(rhs);
                        let h = self.g.helper(Helper::Equal(t));
                        let r = self.func(h);
                        let inst = self.b.ins().call(r, &[a, b]);
                        let eq = self
                            .b
                            .inst_results(inst)
                            .first()
                            .copied()
                            .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                        if *op == mir::CmpOp::NotEq {
                            self.b.ins().bxor_imm_s(eq, 1)
                        } else {
                            eq
                        }
                    }
                    _ => {
                        let a = self.value(lhs);
                        let b = self.value(rhs);
                        let cc = match op {
                            mir::CmpOp::Eq => IntCC::Equal,
                            mir::CmpOp::NotEq => IntCC::NotEqual,
                            mir::CmpOp::Lt => IntCC::SignedLessThan,
                            mir::CmpOp::LtEq => IntCC::SignedLessThanOrEqual,
                            mir::CmpOp::Gt => IntCC::SignedGreaterThan,
                            mir::CmpOp::GtEq => IntCC::SignedGreaterThanOrEqual,
                        };
                        self.b.ins().icmp(cc, a, b)
                    }
                };
                self.set(dst, r);
            }
            mir::Rvalue::Call { callee, args } => {
                let Some(fid) = self.g.funcs.get(callee).copied() else {
                    self.g.defect(self.at, "a call to an undeclared function");
                    return;
                };
                let mut vals = Vec::with_capacity(args.len() + 1);
                if in_memory(dt) {
                    let d = self.addr(dst);
                    vals.push(d);
                }
                for a in args {
                    let v = self.argument(a);
                    vals.push(v);
                }
                let r = self.func(fid);
                let inst = self.b.ins().call(r, &vals);
                let result = self.b.inst_results(inst).first().copied();
                // A callee that failed returned a stand-in; this is where that is noticed. A
                // foreign callee cannot fail the Nazm way, so nothing is checked (N42).
                if !self
                    .g
                    .native
                    .sigs
                    .get(callee.index())
                    .is_some_and(|s| s.foreign)
                {
                    self.check_failed();
                }
                if !in_memory(dt) {
                    if let Some(v) = result {
                        self.set(dst, v);
                    }
                }
            }
            mir::Rvalue::Primitive { op, elem, args } => {
                self.primitive(dst, *op, elem.map(Ty::of), args);
            }
            mir::Rvalue::Closure { func, captures } => {
                let Some(fid) = self.g.funcs.get(func).copied() else {
                    self.g
                        .defect(self.at, "a closure of an undeclared function");
                    return;
                };
                let carried: Vec<Ty> = self
                    .g
                    .native
                    .program
                    .function(*func)
                    .and_then(|g| g.closure.as_ref())
                    .map(|c| c.iter().map(|t| Ty::of(*t)).collect())
                    .unwrap_or_default();
                let (offsets, size) = self.closure_layout(&carried);
                let r = self.func(fid);
                let code = self.b.ins().func_addr(self.g.ptr, r);
                let release = if carried.iter().any(|t| self.g.lay.owns(*t)) {
                    let h = self.g.helper(Helper::ClosureRelease(*func));
                    let hr = self.func(h);
                    self.b.ins().func_addr(self.g.ptr, hr)
                } else {
                    self.iconst(0)
                };
                let n = self.iconst(i64::from(size));
                self.g.needs.closures = true;
                let c = self
                    .rt("nz.closure_new", &[n, code, release])
                    .unwrap_or_else(|| self.iconst(0));
                let none = self.b.ins().icmp_imm_s(IntCC::Equal, c, 0);
                self.guard(none, "N0406", "out of memory");
                for ((o, t), off) in captures.iter().zip(&carried).zip(&offsets) {
                    let at = self.b.ins().iadd_imm_s(c, i64::from(*off));
                    // The object's own reference to each capture.
                    if in_memory(*t) {
                        let src = self.place(o);
                        let n = self.size(*t);
                        self.copy(at, src, n);
                        self.retain_at(at, *t);
                    } else {
                        let v = self.value(o);
                        self.b.ins().store(mem(), v, at, 0);
                        if self.g.lay.owns(*t) {
                            self.retain_value(v, *t);
                        }
                    }
                }
                self.set(dst, c);
            }
            mir::Rvalue::Capture { env, index } => {
                let carried = self.carried.clone();
                let (offsets, _) = self.closure_layout(&carried);
                let (Some(t), Some(off)) = (
                    carried.get(*index as usize).copied(),
                    offsets.get(*index as usize).copied(),
                ) else {
                    self.g
                        .defect(self.at, "a capture the closure does not carry");
                    return;
                };
                let c = self.get(*env);
                let at = self.b.ins().iadd_imm_s(c, i64::from(off));
                self.read_owned(dst, at, t);
            }
            mir::Rvalue::CallIndirect { callee, args } => {
                let c = self.value(callee);
                let fn_ty = self.operand_ty(callee);
                let Ty::Fn(id) = fn_ty else {
                    self.g
                        .defect(self.at, "a call through something not a function value");
                    return;
                };
                let Some(sig) = self
                    .g
                    .native
                    .program
                    .types
                    .function(nazm_sema::FnTypeId::from_index(id.0 as usize))
                    .cloned()
                else {
                    self.g.defect(self.at, "a function type with no signature");
                    return;
                };
                let mut params = vec![fn_ty];
                params.extend(sig.params.iter().map(|t| Ty::of(*t)));
                let signature = self.g.signature(&params, Ty::of(sig.ret));
                let sref = self.b.import_signature(signature);
                let code = self.b.ins().load(types::I64, mem(), c, 8);
                let mut vals = Vec::with_capacity(args.len() + 2);
                if in_memory(dt) {
                    let d = self.addr(dst);
                    vals.push(d);
                }
                vals.push(c);
                for a in args {
                    let v = self.argument(a);
                    vals.push(v);
                }
                let inst = self.b.ins().call_indirect(sref, code, &vals);
                let result = self.b.inst_results(inst).first().copied();
                self.check_failed();
                if !in_memory(dt) {
                    if let Some(v) = result {
                        self.set(dst, v);
                    }
                }
            }
            mir::Rvalue::Record { record, fields } => {
                let d = self.addr(dst);
                for (field, tmp) in fields {
                    let (off, _) = self.field_offset(mir::FieldRef {
                        record: *record,
                        field: *field,
                    });
                    let at = self.b.ins().iadd_imm_s(d, i64::from(off));
                    self.move_into(at, *tmp);
                }
            }
            mir::Rvalue::Variant {
                enumeration,
                variant,
                fields,
            } => {
                let d = self.addr(dst);
                let tag = self.tag(*enumeration, *variant);
                let t = self.iconst(i64::from(tag));
                self.b.ins().store(mem(), t, d, 0);
                for (field, tmp) in fields {
                    let (off, _) = self.payload_offset(*enumeration, tag, *field);
                    let at = self.b.ins().iadd_imm_s(d, i64::from(off));
                    self.move_into(at, *tmp);
                }
            }
            mir::Rvalue::Field { base, field } => {
                let b = self.place(base);
                let (off, t) = self.field_offset(*field);
                let at = self.b.ins().iadd_imm_s(b, i64::from(off));
                self.read_owned(dst, at, t);
            }
            mir::Rvalue::Payload {
                base,
                enumeration,
                variant,
                field,
            } => {
                let b = self.addr(*base);
                let tag = self.tag(*enumeration, *variant);
                let (off, t) = self.payload_offset(*enumeration, tag, *field);
                let at = self.b.ins().iadd_imm_s(b, i64::from(off));
                self.read_owned(dst, at, t);
            }
        }
    }

    fn arith(&mut self, op: mir::ArithOp, x: Value, y: Value) -> Value {
        match op {
            mir::ArithOp::Add => {
                let (r, of) = self.b.ins().sadd_overflow(x, y);
                self.guard(of, "N0400", "this addition overflows `Int`");
                r
            }
            mir::ArithOp::Sub => {
                let (r, of) = self.b.ins().ssub_overflow(x, y);
                self.guard(of, "N0400", "this subtraction overflows `Int`");
                r
            }
            mir::ArithOp::Mul => {
                let (r, of) = self.b.ins().smul_overflow(x, y);
                self.guard(of, "N0400", "this multiplication overflows `Int`");
                r
            }
            mir::ArithOp::Div => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.guard(zero, "N0401", "division by zero");
                let special = self.min_over_minus_one(x, y);
                self.guard(special, "N0400", "this division overflows `Int`");
                self.b.ins().sdiv(x, y)
            }
            mir::ArithOp::Rem => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.guard(zero, "N0401", "division by zero");
                // `Int::MIN % -1` is 0 and never reaches `srem`, which faults on that pair.
                let special = self.min_over_minus_one(x, y);
                let compute = self.b.create_block();
                let join = self.b.create_block();
                self.b.append_block_param(join, types::I64);
                let z = self.iconst(0);
                self.b.ins().brif(special, join, &[z.into()], compute, &[]);
                self.b.switch_to_block(compute);
                let r = self.b.ins().srem(x, y);
                self.b.ins().jump(join, &[r.into()]);
                self.b.switch_to_block(join);
                self.b.block_params(join).first().copied().unwrap_or(z)
            }
        }
    }

    fn min_over_minus_one(&mut self, x: Value, y: Value) -> Value {
        let is_min = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
        let is_m1 = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
        self.b.ins().band(is_min, is_m1)
    }

    /// Whether the strings at two addresses have the same bytes: equal lengths, then `memcmp`
    /// only when they are.
    pub(crate) fn string_equal(&mut self, a: Value, b: Value) -> Value {
        let la = self.b.ins().load(types::I64, mem(), a, 8);
        let lb = self.b.ins().load(types::I64, mem(), b, 8);
        let same = self.b.ins().icmp(IntCC::Equal, la, lb);
        let compare = self.b.create_block();
        let join = self.b.create_block();
        self.b.append_block_param(join, types::I8);
        let no = self.b.ins().iconst(types::I8, 0);
        self.b.ins().brif(same, compare, &[], join, &[no.into()]);
        self.b.switch_to_block(compare);
        let pa = self.b.ins().load(types::I64, mem(), a, 0);
        let pb = self.b.ins().load(types::I64, mem(), b, 0);
        let diff = self.rt("memcmp", &[pa, pb, la]).unwrap_or(no);
        let eq = self.b.ins().icmp_imm_s(IntCC::Equal, diff, 0);
        self.b.ins().jump(join, &[eq.into()]);
        self.b.switch_to_block(join);
        self.b.block_params(join).first().copied().unwrap_or(no)
    }

    fn spawn(&mut self, list: mir::LocalId, callee: mir::FuncId, args: &[mir::Operand]) {
        self.g.needs.tasks = true;
        self.g.needs.sequences = true;
        // The block: the thread handle, the task's failure message and length, then each
        // argument at its alignment.
        let mut at = 24u32;
        let mut shape = Vec::with_capacity(args.len());
        for a in args {
            let t = self.operand_ty(a);
            let l = self
                .g
                .lay
                .lay(t)
                .unwrap_or(crate::layout::Lay { size: 8, align: 8 });
            at = at.div_ceil(l.align.max(1)) * l.align.max(1);
            shape.push((t, at));
            at += l.size;
        }
        let size = self.iconst(i64::from(at.max(24)));
        let blk = self.rt("malloc", &[size]).unwrap_or_else(|| self.iconst(0));
        let null = self.b.ins().icmp_imm_s(IntCC::Equal, blk, 0);
        self.guard(null, "N0406", "out of memory");
        let z = self.iconst(0);
        self.b.ins().store(mem(), z, blk, 8);
        self.b.ins().store(mem(), z, blk, 16);
        // The block takes a reference of its own to each argument; the task gives it back.
        for (a, (t, off)) in args.iter().zip(&shape) {
            let dst = self.b.ins().iadd_imm_s(blk, i64::from(*off));
            if in_memory(*t) {
                let src = self.place(a);
                let n = self.size(*t);
                self.copy(dst, src, n);
                self.retain_at(dst, *t);
            } else {
                let v = self.value(a);
                if self.g.lay.owns(*t) {
                    self.retain_value(v, *t);
                }
                self.b.ins().store(mem(), v, dst, 0);
            }
        }
        let tramp = self.g.trampoline(callee, shape.clone());
        let r = self.func(tramp);
        let fa = self.b.ins().func_addr(self.g.ptr, r);
        let tids = self.get(list);
        let rc = self
            .rt("nz.spawn", &[tids, fa, blk])
            .unwrap_or_else(|| self.b.ins().iconst(types::I32, 0));
        let started = self.b.ins().icmp_imm_s(IntCC::Equal, rc, 0);
        let cleanup = self.b.create_block();
        let after = self.b.create_block();
        self.b.ins().brif(started, after, &[], cleanup, &[]);
        self.b.switch_to_block(cleanup);
        // A task that never started owns nothing: give back what was packed for it.
        for (t, off) in &shape {
            let at = self.b.ins().iadd_imm_s(blk, i64::from(*off));
            self.release_at(at, *t);
        }
        self.rt("free", &[blk]);
        self.b.ins().jump(after, &[]);
        self.b.switch_to_block(after);
        let oom = self.b.ins().icmp_imm_s(IntCC::Equal, rc, -1);
        self.guard(oom, "N0406", "out of memory");
        let bad = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, rc, 0);
        self.guard(bad, "N0404", "this task could not be started");
    }

    // --- terminators ---------------------------------------------------------------------------

    fn terminator(&mut self, t: &mir::Terminator, ret: Ty) {
        match t {
            mir::Terminator::Goto(b) => {
                let blk = self.block(*b);
                self.b.ins().jump(blk, &[]);
            }
            mir::Terminator::Branch {
                cond,
                then,
                otherwise,
            } => {
                let c = self.value(cond);
                let (a, b) = (self.block(*then), self.block(*otherwise));
                self.b.ins().brif(c, a, &[], b, &[]);
            }
            mir::Terminator::Switch {
                scrutinee,
                enumeration,
                arms,
            } => {
                let a = self.addr(*scrutinee);
                let tag = self.b.ins().load(types::I64, mem(), a, 0);
                let mut sw = Switch::new();
                for (v, b) in arms {
                    let t = self.tag(*enumeration, *v);
                    let blk = self.block(*b);
                    sw.set_entry(u128::from(t), blk);
                }
                // Every variant has an arm, so the default is unreachable for a value this
                // program built; it is the first arm rather than a trap, so a compiler
                // mistake is a wrong answer and not undefined behaviour.
                let default = match arms.first() {
                    Some((_, b)) => self.block(*b),
                    None => self.b.create_block(),
                };
                sw.emit(&mut self.b, tag, default);
            }
            mir::Terminator::Return(l) => {
                if let Some(out) = self.sret {
                    let src = self.addr(*l);
                    let n = self.size(ret);
                    self.copy(out, src, n);
                    self.b.ins().return_(&[]);
                } else {
                    let v = self.get(*l);
                    self.b.ins().return_(&[v]);
                }
            }
            mir::Terminator::Unwind => {
                // The failure exit: a stand-in the caller never reads, because its failure
                // check leaves first.
                if self.sret.is_some() {
                    self.b.ins().return_(&[]);
                } else {
                    let z = self.b.ins().iconst(Gen::clif(ret), 0);
                    self.b.ins().return_(&[z]);
                }
            }
            mir::Terminator::Exit(code) => {
                let c = self.value(code);
                let narrow = self.b.ins().ireduce(types::I32, c);
                self.rt("exit", &[narrow]);
                self.b.ins().trap(TrapCode::unwrap_user(1));
            }
        }
    }

    fn block(&mut self, b: mir::BlockId) -> Block {
        if let Some(blk) = self.blocks.get(b.index()).copied() {
            blk
        } else {
            self.g
                .defect(self.at, "a jump to a block MIR does not have");
            self.b.create_block()
        }
    }
}
