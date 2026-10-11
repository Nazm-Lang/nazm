//! One MIR function, as LIR.
//!
//! Each MIR block is a LIR block. A local of a scalar type is a variable; a `Str`, a record or an
//! enum a stack slot, or — a parameter — the caller's memory it borrows, by its pointer. A drop, a
//! copy and a move are the calls, copies and stores MIR says they are; a failure edge is a jump to
//! the LIR block of the MIR block it names.

use nazm_mir as mir;
use nazm_span::Span;

use super::ir::{
    BinOp, Class, Cmp, DebugVar, Function, Inst, Sig, Storage, SymId, Term, Var, B, V,
};
use super::lower::{class, in_memory, rt, Fb, Helper, Lw};
use crate::ir::{EnumTy, RecordTy, Ty};
use crate::lower::Sig as NativeSig;

/// Where a MIR local lives.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Loc {
    Var(Var, Ty),
    /// A scalar temporary assigned once and read only later in its block (N59, §7.61): no
    /// variable, the value its one assignment produced.
    Value(usize, Ty),
    /// In a stack slot, or — a parameter — in memory the caller owns, by its pointer.
    Mem(MemAt, Ty),
    Tasks(Var),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MemAt {
    Slot(super::ir::Slot),
    Borrowed(V),
}

pub(crate) struct Fx<'l, 'a> {
    pub lw: &'l mut Lw<'a>,
    pub b: Fb,
    pub locals: Vec<Loc>,
    blocks: Vec<B>,
    sret: Option<V>,
    /// The block the statement being lowered fails to.
    pub unwind: Option<B>,
    pub at: Span,
    /// What the closure object of the function being lowered carries, if it is a function
    /// value's code (N50).
    carried: Vec<Ty>,
    /// The values of promoted locals, by local.
    promoted: Vec<Option<V>>,
    /// In a debug build, the variable holding each borrowed parameter's address.
    borrowed_at: std::collections::BTreeMap<usize, Var>,
    /// Where a function whose own frame is large checks its stack (Q1-A1-01): set by its
    /// `StackCheck`, so the function is entered through [`frame_entry`].
    frame_at: Option<Span>,
}

/// Lower one MIR function.
pub(crate) fn lower(
    lw: &mut Lw<'_>,
    f: &mir::Function,
    sig: &NativeSig,
    sym: SymId,
) -> Vec<Function> {
    let mut fx = Fx {
        lw,
        b: Fb::new(),
        locals: Vec::new(),
        blocks: Vec::new(),
        sret: None,
        unwind: None,
        at: f.span,
        carried: f.closure.iter().flatten().map(|t| Ty::of(*t)).collect(),
        promoted: vec![None; f.locals.len()],
        borrowed_at: std::collections::BTreeMap::new(),
        frame_at: None,
    };
    let promotable = promotable(f);
    let mut next_param = 0u32;
    if in_memory(sig.ret) {
        fx.sret = Some(fx.param(0));
        next_param = 1;
    }
    for (i, l) in f.locals.iter().enumerate() {
        let is_param = i < f.params as usize;
        let loc = match l.ty.value().map(Ty::of) {
            None => {
                let v = fx.b.var(Class::Ptr);
                let z = fx.iconst_c(Class::Ptr, 0);
                fx.b.push(Inst::SetVar { var: v, src: z });
                Loc::Tasks(v)
            }
            Some(t) if in_memory(t) => {
                if is_param {
                    let p = fx.param(next_param);
                    next_param += 1;
                    // Where a debugger finds it: its address, in a variable of its own.
                    if fx.lw.debug {
                        let v = fx.b.var(Class::Ptr);
                        fx.set_var(v, p);
                        fx.borrowed_at.insert(i, v);
                    }
                    Loc::Mem(MemAt::Borrowed(p), t)
                } else {
                    let lay = fx.lw.lay.lay(t).unwrap_or(crate::abi::Lay::new(8, 8));
                    let slot = fx.b.slot(lay.size, lay.align.max(8));
                    if let Some(n) = fx.b.slot_names.last_mut() {
                        *n = format!("{} : {}", l.name, t.name());
                    }
                    if fx.lw.lay.owns(t) {
                        let a = fx.slot_addr(slot);
                        fx.b.push(Inst::Zero {
                            dst: a,
                            size: lay.size,
                        });
                    }
                    Loc::Mem(MemAt::Slot(slot), t)
                }
            }
            Some(t) if promotable.get(i).copied().unwrap_or(false) => Loc::Value(i, t),
            Some(t) => {
                let v = fx.b.var(class(t));
                if let Some(n) = fx.b.var_names.last_mut() {
                    *n = format!("{} : {}", l.name, t.name());
                }
                let init = if is_param {
                    let p = fx.param(next_param);
                    next_param += 1;
                    p
                } else {
                    fx.iconst_c(class(t), 0)
                };
                fx.b.push(Inst::SetVar { var: v, src: init });
                Loc::Var(v, t)
            }
        };
        fx.locals.push(loc);
        // A variable a debugger can read (N58): a parameter or a source binding of a type it
        // is told about.
        fx.debug_var(i, l);
    }
    for _ in &f.blocks {
        let blk = fx.b.block();
        fx.blocks.push(blk);
    }
    if let Some(first) = fx.blocks.first().copied() {
        fx.b.term(Term::Jump(first));
    }
    for (i, blk) in f.blocks.iter().enumerate() {
        let Some(lb) = fx.blocks.get(i).copied() else {
            continue;
        };
        fx.b.switch(lb);
        for s in &blk.stmts {
            fx.at = s.at;
            fx.locate(s.at);
            fx.unwind = s
                .kind
                .unwind()
                .and_then(|u| fx.blocks.get(u.index()).copied());
            fx.statement(&s.kind);
        }
        fx.at = blk.term_at;
        fx.locate(blk.term_at);
        fx.terminator(&blk.term, sig.ret);
    }
    let at = fx
        .lw
        .debug
        .then(|| {
            fx.lw
                .names
                .position(f.span)
                .map(|(file, line, _)| (file, line))
        })
        .flatten();
    let frame_at = fx.frame_at;
    let mut lowered = fx.b.finish(sym, f.name.clone(), at);
    lowered.section.clone_from(&f.section);
    let Some(at) = frame_at else {
        return vec![lowered];
    };
    // A large frame is checked before it exists (Q1-A1-01): the function's prologue lays out —
    // and probes — its whole frame before its first statement runs, so a check there comes too
    // late. Its body moves to a symbol of its own, and the function's symbol becomes an entry
    // with a small frame that asks for room for the body's frame, then calls it.
    let frame: u64 = lowered
        .slots
        .iter()
        .map(|s| u64::from(s.size).next_multiple_of(u64::from(s.align.max(1))))
        .fold(0u64, u64::saturating_add);
    let Some(sig) = lw.sig_of(sym) else {
        return vec![lowered];
    };
    let body = lw.body_symbol(sig.clone());
    lowered.sym = body;
    let mut entry = frame_entry(lw, sym, body, &sig, frame, at);
    entry.section.clone_from(&f.section);
    vec![entry, lowered]
}

/// The entry of a function whose frame is large (Q1-A1-01): its parameters passed on unchanged
/// to its body once the thread's stack has room for the body's `frame`, and `N0408` at the
/// function's name, with the failure exit's stand-in, when it has not.
fn frame_entry(
    lw: &mut Lw<'_>,
    sym: SymId,
    body: SymId,
    sig: &Sig,
    frame: u64,
    at: Span,
) -> Function {
    let mut fx = Fx::helper(lw);
    fx.at = at;
    let args: Vec<V> = (0..sig.params.len())
        .map(|i| fx.param(u32::try_from(i).unwrap_or(0)))
        .collect();
    let need = fx.iconst(i64::try_from(frame).unwrap_or(i64::MAX));
    let past = fx.rt_v("nz.stack_check_frame", &[need], Class::I32);
    let bad = fx.cmp_imm(Cmp::Ne, past, Class::I32, 0);
    let failing = fx.b.block();
    let enter = fx.b.block();
    fx.branch(bad, failing, enter);
    fx.b.switch(failing);
    let (d, n) = fx.lw.message("N0408", crate::STACK_EXHAUSTED, at);
    let p = fx.addr_of(d);
    let len = fx.iconst(i64::try_from(n).unwrap_or(0));
    fx.rt("nz.fail", &[p, len]);
    // The failure exit's stand-in, which the caller never reads (`Terminator::Unwind`).
    let z = sig.ret.map(|c| fx.iconst_c(c, 0));
    fx.b.term(Term::Return(z));
    fx.b.switch(enter);
    let r = fx.call(body, args);
    fx.b.term(Term::Return(r));
    fx.b.finish(sym, "a large frame's entry".to_owned(), None)
}

impl<'l, 'a> Fx<'l, 'a> {
    /// A context for a generated function: no MIR locals, blocks or failure edge.
    pub(crate) fn helper(lw: &'l mut Lw<'a>) -> Self {
        Self {
            lw,
            b: Fb::new(),
            locals: Vec::new(),
            blocks: Vec::new(),
            sret: None,
            unwind: None,
            at: Span::synthetic(),
            carried: Vec::new(),
            promoted: Vec::new(),
            borrowed_at: std::collections::BTreeMap::new(),
        frame_at: None,
        }
    }

    fn debug_var(&mut self, i: usize, local: &mir::LocalDecl) {
        if !self.lw.debug {
            return;
        }
        let kind = match local.ty.value().map(Ty::of) {
            Some(Ty::Int) => "int",
            Some(Ty::Bool) => "bool",
            Some(Ty::Str) => "str",
            _ => return,
        };
        let arg = match local.origin {
            mir::LocalOrigin::Param => u32::try_from(i + 1).unwrap_or(0),
            mir::LocalOrigin::Binding { .. } => 0,
            mir::LocalOrigin::Temp => return,
        };
        let Some((_, line, _)) = self.lw.names.position(local.at) else {
            return;
        };
        if local.name.is_empty()
            || !local
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return;
        }
        let storage = match self.locals.get(i).copied() {
            Some(Loc::Var(v, _)) => Storage::Var(v),
            Some(Loc::Mem(MemAt::Slot(s), _)) => Storage::Slot(s),
            Some(Loc::Mem(MemAt::Borrowed(_), _)) => match self.borrowed_at.get(&i) {
                Some(v) => Storage::Indirect(*v),
                None => return,
            },
            _ => return,
        };
        // Its scope (N91): the block's first and last positions, or zeros for the whole
        // function.
        let names = self.lw.names;
        let scope = local
            .scope
            .and_then(|b| {
                let first = names.position(b)?;
                let last = names.position(Span::new(b.file, b.end.saturating_sub(1), b.end))?;
                Some((first.1, first.2, last.1, last.2))
            })
            .unwrap_or((0, 0, 0, 0));
        self.b.debug_vars.push(DebugVar {
            storage,
            arg,
            line,
            kind,
            scope,
            name: local.name.clone(),
        });
    }

    fn locate(&mut self, at: Span) {
        if let Some(p) = self.lw.pos(at) {
            self.b.push(Inst::Loc(p));
        }
    }

    // --- instructions ----------------------------------------------------------------------

    pub(crate) fn param(&mut self, index: u32) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Param { dst, index });
        dst
    }

    pub(crate) fn iconst_c(&mut self, class: Class, value: i64) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Const { dst, class, value });
        dst
    }

    pub(crate) fn iconst(&mut self, value: i64) -> V {
        self.iconst_c(Class::I64, value)
    }

    pub(crate) fn bit(&mut self, value: bool) -> V {
        self.iconst_c(Class::Bit, i64::from(value))
    }

    pub(crate) fn null(&mut self) -> V {
        self.iconst_c(Class::Ptr, 0)
    }

    pub(crate) fn addr_of(&mut self, sym: SymId) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Addr { dst, sym });
        dst
    }

    pub(crate) fn slot_addr(&mut self, slot: super::ir::Slot) -> V {
        let dst = self.b.value();
        self.b.push(Inst::SlotAddr { dst, slot });
        dst
    }

    /// A fresh slot of `size` bytes, its address.
    pub(crate) fn scratch(&mut self, size: u32) -> V {
        let s = self.b.slot(size.max(8), 8);
        self.slot_addr(s)
    }

    pub(crate) fn load(&mut self, class: Class, addr: V, offset: i32) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Load {
            dst,
            class,
            addr,
            offset,
        });
        dst
    }

    pub(crate) fn store(&mut self, addr: V, offset: i32, value: V) {
        self.b.push(Inst::Store {
            addr,
            offset,
            value,
        });
    }

    pub(crate) fn bin(&mut self, op: BinOp, a: V, b: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Bin { dst, op, a, b });
        dst
    }

    pub(crate) fn add_imm(&mut self, a: V, k: i64) -> V {
        let c = self.iconst(k);
        self.bin(BinOp::Add, a, c)
    }

    pub(crate) fn cmp(&mut self, op: Cmp, a: V, b: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Cmp { dst, op, a, b });
        dst
    }

    /// Compare an integer value with a constant of its class.
    pub(crate) fn cmp_imm(&mut self, op: Cmp, a: V, class: Class, k: i64) -> V {
        let c = self.iconst_c(class, k);
        self.cmp(op, a, c)
    }

    /// Whether a pointer is null.
    pub(crate) fn is_null(&mut self, p: V) -> V {
        let z = self.null();
        self.cmp(Cmp::Eq, p, z)
    }

    pub(crate) fn or(&mut self, a: V, b: V) -> V {
        self.bin(BinOp::Or, a, b)
    }

    pub(crate) fn select(&mut self, cond: V, a: V, b: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Select { dst, cond, a, b });
        dst
    }

    pub(crate) fn ext(&mut self, to: Class, src: V, signed: bool) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Ext {
            dst,
            to,
            src,
            signed,
        });
        dst
    }

    pub(crate) fn trunc(&mut self, to: Class, src: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::Trunc { dst, to, src });
        dst
    }

    /// `base + offset` for an `I64` offset.
    pub(crate) fn ptr_add(&mut self, base: V, offset: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::PtrAdd { dst, base, offset });
        dst
    }

    pub(crate) fn ptr_off(&mut self, base: V, offset: i64) -> V {
        if offset == 0 {
            return base;
        }
        let k = self.iconst(offset);
        self.ptr_add(base, k)
    }

    pub(crate) fn ptr_to_int(&mut self, src: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::PtrToInt { dst, src });
        dst
    }

    pub(crate) fn int_to_ptr(&mut self, src: V) -> V {
        let dst = self.b.value();
        self.b.push(Inst::IntToPtr { dst, src });
        dst
    }

    pub(crate) fn get_var(&mut self, var: Var) -> V {
        let dst = self.b.value();
        self.b.push(Inst::GetVar { dst, var });
        dst
    }

    pub(crate) fn set_var(&mut self, var: Var, src: V) {
        self.b.push(Inst::SetVar { var, src });
    }

    pub(crate) fn call(&mut self, callee: SymId, args: Vec<V>) -> Option<V> {
        let returns = self.lw_sig(callee).and_then(|s| s.ret).is_some();
        let dst = returns.then(|| self.b.value());
        self.b.push(Inst::Call { dst, callee, args });
        dst
    }

    fn lw_sig(&self, s: SymId) -> Option<Sig> {
        self.lw.sig_of(s)
    }

    /// Call a runtime entry point from the registry.
    pub(crate) fn rt(&mut self, name: &'static str, args: &[V]) -> Option<V> {
        let at = self.at;
        let s = self.lw.runtime(name, at);
        self.call(s, args.to_vec())
    }

    /// A runtime entry's result, or a zero of `class` if it has none (a registry defect, which
    /// the lowering has already reported).
    pub(crate) fn rt_v(&mut self, name: &'static str, args: &[V], class: Class) -> V {
        match self.rt(name, args) {
            Some(v) => v,
            None => self.iconst_c(class, 0),
        }
    }

    pub(crate) fn copy(&mut self, dst: V, src: V, size: u32) {
        self.b.push(Inst::Copy { dst, src, size });
    }

    pub(crate) fn zero(&mut self, dst: V, size: u32) {
        self.b.push(Inst::Zero { dst, size });
    }

    pub(crate) fn jump(&mut self, to: B) {
        self.b.term(Term::Jump(to));
    }

    pub(crate) fn branch(&mut self, cond: V, then: B, otherwise: B) {
        self.b.term(Term::Branch {
            cond,
            then,
            otherwise,
        });
    }

    pub(crate) fn size(&self, t: Ty) -> u32 {
        self.lw.size(t)
    }

    pub(crate) fn ty(&self, l: mir::LocalId) -> Ty {
        match self.locals.get(l.index()) {
            Some(Loc::Var(_, t) | Loc::Mem(_, t) | Loc::Value(_, t)) => *t,
            _ => Ty::Int,
        }
    }

    /// The address of an in-memory local.
    pub(crate) fn addr(&mut self, l: mir::LocalId) -> V {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Mem(MemAt::Slot(s), _)) => self.slot_addr(s),
            Some(Loc::Mem(MemAt::Borrowed(p), _)) => p,
            _ => {
                let at = self.at;
                self.lw
                    .defect(at, "an address of a local held in a register");
                self.null()
            }
        }
    }

    /// The value of a scalar local.
    pub(crate) fn get(&mut self, l: mir::LocalId) -> V {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Var(v, _) | Loc::Tasks(v)) => self.get_var(v),
            Some(Loc::Value(i, t)) => match self.promoted.get(i).copied().flatten() {
                Some(v) => v,
                None => self.iconst_c(class(t), 0),
            },
            _ => {
                let at = self.at;
                self.lw
                    .defect(at, "a scalar read of a local held in memory");
                self.iconst(0)
            }
        }
    }

    pub(crate) fn set(&mut self, l: mir::LocalId, v: V) {
        match self.locals.get(l.index()).copied() {
            Some(Loc::Var(var, _) | Loc::Tasks(var)) => self.set_var(var, v),
            Some(Loc::Value(i, _)) => {
                if let Some(slot) = self.promoted.get_mut(i) {
                    *slot = Some(v);
                }
            }
            _ => {
                let at = self.at;
                self.lw
                    .defect(at, "a scalar write of a local held in memory");
            }
        }
    }

    /// Empty a scalar local: a zero of its class.
    fn clear(&mut self, l: mir::LocalId) {
        let c = match self.locals.get(l.index()).copied() {
            Some(Loc::Var(_, t) | Loc::Value(_, t)) => class(t),
            _ => Class::Ptr,
        };
        let z = self.iconst_c(c, 0);
        self.set(l, z);
    }

    /// An operand as a scalar value.
    pub(crate) fn value(&mut self, o: &mir::Operand) -> V {
        match o {
            mir::Operand::Local(l) => self.get(*l),
            mir::Operand::Const(mir::Const::Int(v)) => self.iconst(*v),
            mir::Operand::Const(mir::Const::Num { kind, bits }) => self.num_const(*kind, *bits),
            mir::Operand::Const(mir::Const::Bool(v)) => self.bit(*v),
            mir::Operand::Const(mir::Const::Str(_) | mir::Const::Cell { .. }) => {
                let at = self.at;
                self.lw
                    .defect(at, "a string literal or a static atomic read into a scalar");
                self.iconst(0)
            }
        }
    }

    /// An operand as the address of its value.
    pub(crate) fn place(&mut self, o: &mir::Operand) -> V {
        match o {
            mir::Operand::Local(l) => self.addr(*l),
            mir::Operand::Const(mir::Const::Str(bytes)) => {
                let d = self.lw.str_value(bytes);
                self.addr_of(d)
            }
            // A static atomic (Gate 3): its cell's triple, as a literal's is.
            mir::Operand::Const(mir::Const::Cell { module, name }) => {
                if let Some(d) = self.lw.cell_value(*module, name) {
                    self.addr_of(d)
                } else {
                    let at = self.at;
                    self.lw.defect(at, "a static the program does not declare");
                    self.null()
                }
            }
            mir::Operand::Const(_) => {
                let at = self.at;
                self.lw.defect(at, "the address of a scalar constant");
                self.null()
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
    pub(crate) fn argument(&mut self, o: &mir::Operand) -> V {
        if in_memory(self.operand_ty(o)) {
            self.place(o)
        } else {
            self.value(o)
        }
    }

    // --- failure -------------------------------------------------------------------------

    /// Record the failure and leave along the statement's failure edge.
    pub(crate) fn fail(&mut self, code: &str, message: &str) {
        let at = self.at;
        let (d, n) = self.lw.message(code, message, at);
        let p = self.addr_of(d);
        let len = self.iconst(i64::try_from(n).unwrap_or(0));
        self.rt("nz.fail", &[p, len]);
        self.leave();
    }

    /// Jump along the failure edge.
    pub(crate) fn leave(&mut self) {
        match self.unwind {
            Some(u) => self.jump(u),
            None => {
                let at = self.at;
                self.lw.defect(at, "a failure with no failure edge in MIR");
                self.b.term(Term::Unreachable);
            }
        }
    }

    /// Fail with this diagnostic when `bad` is true; continue in a fresh block otherwise.
    pub(crate) fn guard(&mut self, bad: V, code: &str, message: &str) {
        let failing = self.b.block();
        let cont = self.b.block();
        self.branch(bad, failing, cont);
        self.b.switch(failing);
        self.fail(code, message);
        self.b.switch(cont);
    }

    /// A guard before a foreign call (N55): the C-string and record copies already made for
    /// the call are freed on the failing path.
    fn foreign_guard(&mut self, bad: V, code: &str, message: &str, copies: &[V]) {
        let failing = self.b.block();
        let cont = self.b.block();
        self.branch(bad, failing, cont);
        self.b.switch(failing);
        for c in copies {
            self.rt("free", &[*c]);
        }
        self.fail(code, message);
        self.b.switch(cont);
    }

    /// Leave along the failure edge if this thread has failed.
    pub(crate) fn check_failed(&mut self) {
        let f = self.rt_v("nz.failed_now", &[], Class::I32);
        let failed = self.cmp_imm(Cmp::Ne, f, Class::I32, 0);
        let cont = self.b.block();
        match self.unwind {
            Some(u) => self.branch(failed, u, cont),
            None => {
                let at = self.at;
                self.lw.defect(at, "a failure check with no failure edge");
                self.jump(cont);
            }
        }
        self.b.switch(cont);
    }

    // --- ownership -----------------------------------------------------------------------

    /// Take a reference to what the value of type `t` at `addr` owns.
    pub(crate) fn retain_at(&mut self, addr: V, t: Ty) {
        if !self.lw.lay.owns(t) {
            return;
        }
        match t {
            Ty::Str => {
                let [_, _, owner] = self.lw.data.str_fields();
                let o = self.load(Class::Ptr, addr, offset(owner));
                self.rt("nz.str_retain", &[o]);
            }
            Ty::Record(_) | Ty::Enum(_) => {
                let h = self.lw.helper(Helper::Retain(t));
                self.call(h, vec![addr]);
            }
            _ => {
                let v = self.load(Class::Ptr, addr, 0);
                self.retain_value(v, t);
            }
        }
    }

    /// Take a reference to what a handle owns.
    pub(crate) fn retain_value(&mut self, v: V, t: Ty) {
        match t {
            Ty::Ints | Ty::Strs | Ty::Vec(_) => {
                self.rt("nz.arr_retain", &[v]);
            }
            // A generic channel's count is where an `Int` channel's is (N53).
            Ty::Chan | Ty::ChanOf(_) => {
                self.rt("nz.chan_retain", &[v]);
            }
            Ty::Fn(_) => {
                self.rt("nz.closure_retain", &[v]);
            }
            _ => {}
        }
    }

    /// Give back what the value of type `t` at `addr` owns.
    pub(crate) fn release_at(&mut self, addr: V, t: Ty) {
        if !self.lw.lay.owns(t) {
            return;
        }
        match t {
            Ty::Str => {
                let [_, _, owner] = self.lw.data.str_fields();
                let o = self.load(Class::Ptr, addr, offset(owner));
                self.rt("nz.str_release", &[o]);
            }
            Ty::Record(_) | Ty::Enum(_) => {
                let h = self.lw.helper(Helper::Release(t));
                self.call(h, vec![addr]);
            }
            _ => {
                let v = self.load(Class::Ptr, addr, 0);
                self.release_value(v, t);
            }
        }
    }

    /// Give back what a handle owns.
    pub(crate) fn release_value(&mut self, v: V, t: Ty) {
        match t {
            Ty::Ints => {
                self.rt("nz.seq_release", &[v]);
            }
            Ty::Strs => {
                self.rt("nz.strs_release", &[v]);
            }
            Ty::Chan => {
                self.rt("nz.chan_release", &[v]);
            }
            Ty::ChanOf(_) => {
                self.rt("nz.chanv_release", &[v]);
            }
            Ty::Fn(_) => {
                self.rt("nz.closure_release", &[v]);
            }
            Ty::Vec(id) => {
                let elem = self
                    .lw
                    .unit
                    .vecs
                    .iter()
                    .find(|l| l.id == id)
                    .map_or(Ty::Int, |l| l.elem);
                if self.soa_fields(t).is_some() {
                    // A field-by-field vector (N68): its fields own nothing, and its data is one
                    // allocation, which the sequence release frees.
                    self.rt("nz.seq_release", &[v]);
                } else if self.lw.lay.owns(elem) {
                    let h = self.lw.helper(Helper::ReleaseElement(elem));
                    let f = self.addr_of(h);
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
    pub(crate) fn move_into(&mut self, to: V, from: mir::LocalId) {
        let t = self.ty(from);
        if in_memory(t) {
            let src = self.addr(from);
            let n = self.size(t);
            self.copy(to, src, n);
            if self.lw.lay.owns(t) {
                self.zero(src, n);
            }
        } else {
            let v = self.get(from);
            self.store(to, 0, v);
            if self.lw.lay.owns(t) {
                self.clear(from);
            }
        }
    }

    /// Where each capture sits in a closure object, after its header, and the object's size
    /// (N50): each in an 8-aligned slot of its own size.
    pub(crate) fn closure_layout(&self, carried: &[Ty]) -> (Vec<u32>, u32) {
        let mut off = rt::CLOSURE_CAPTURES;
        let mut offsets = Vec::with_capacity(carried.len());
        for t in carried {
            offsets.push(off);
            off += self.size(*t).div_ceil(8) * 8;
        }
        (offsets, off)
    }

    /// Read a value of type `t` at `addr` into local `dst`, taking a reference of its own.
    fn read_owned(&mut self, dst: mir::LocalId, addr: V, t: Ty) {
        if in_memory(t) {
            let d = self.addr(dst);
            let n = self.size(t);
            self.copy(d, addr, n);
            self.retain_at(d, t);
        } else {
            let v = self.load(class(t), addr, 0);
            if self.lw.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.set(dst, v);
        }
    }

    // --- layout --------------------------------------------------------------------------

    pub(crate) fn field_offset(&mut self, f: mir::FieldRef) -> (u32, Ty) {
        let rid = RecordTy(u32::try_from(f.record.index()).unwrap_or(u32::MAX));
        let pos = self
            .lw
            .unit
            .records
            .iter()
            .find(|r| r.id == rid)
            .and_then(|r| r.of_declaration.get(f.field as usize).copied())
            .unwrap_or(0);
        match self.lw.lay.records.get(&rid.0) {
            Some(s) => (
                s.offsets.get(pos as usize).copied().unwrap_or(0),
                s.fields.get(pos as usize).copied().unwrap_or(Ty::Int),
            ),
            None => {
                let at = self.at;
                self.lw.defect(at, "a record with no layout");
                (0, Ty::Int)
            }
        }
    }

    pub(crate) fn tag(&self, e: nazm_sema::EnumId, variant: u32) -> u32 {
        let eid = EnumTy(u32::try_from(e.index()).unwrap_or(u32::MAX));
        self.lw
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
            .lw
            .unit
            .enums
            .iter()
            .find(|l| l.id == eid)
            .and_then(|l| l.variants.get(tag as usize))
            .and_then(|v| v.of_declaration.get(field as usize).copied())
            .unwrap_or(0);
        match self
            .lw
            .lay
            .enums
            .get(&eid.0)
            .and_then(|s| s.variants.get(tag as usize))
        {
            Some(v) => (
                crate::abi::PAYLOAD + v.offsets.get(pos as usize).copied().unwrap_or(0),
                v.fields.get(pos as usize).copied().unwrap_or(Ty::Int),
            ),
            None => {
                let at = self.at;
                self.lw.defect(at, "an enum with no layout");
                (crate::abi::PAYLOAD, Ty::Int)
            }
        }
    }

    // --- statements ----------------------------------------------------------------------

    fn statement(&mut self, s: &mir::StatementKind) {
        match s {
            mir::StatementKind::Assign { dst, value, .. } => self.assign(*dst, value),
            mir::StatementKind::SetField { base, path, value } => {
                let mut a = self.addr(*base);
                let mut last = Ty::Int;
                for step in path {
                    let (off, t) = self.field_offset(*step);
                    a = self.ptr_off(a, i64::from(off));
                    last = t;
                }
                // The new value already owns its reference, so the old one may go first.
                self.release_at(a, last);
                self.move_into(a, *value);
            }
            mir::StatementKind::SetElement {
                base, path, value, ..
            } => self.set_element(*base, path, *value),
            mir::StatementKind::Drop(l) => {
                let t = self.ty(*l);
                if !self.lw.lay.owns(t) {
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
                    self.clear(*l);
                }
            }
            mir::StatementKind::ScopeOpen { list, .. } => {
                let t = self.rt_v("nz.arr_new", &[], Class::Ptr);
                let null = self.is_null(t);
                self.guard(null, "N0406", "out of memory");
                self.set(*list, t);
            }
            mir::StatementKind::Spawn {
                list, callee, args, ..
            } => self.spawn(*list, *callee, args),
            mir::StatementKind::ScopeJoin { list } => {
                let t = self.get(*list);
                let z = self.null();
                self.set(*list, z);
                let o = self.scratch(16);
                // Counted before the join, while the list still says how many (N54).
                self.rt("nz.sched_note_join", &[t]);
                self.rt("nz.scope_join_to", &[t, o]);
                let msg = self.load(Class::Ptr, o, rt::JOIN_MESSAGE);
                let none = self.is_null(msg);
                let take = self.b.block();
                let cont = self.b.block();
                self.branch(none, cont, take);
                self.b.switch(take);
                let len = self.load(Class::I64, o, rt::JOIN_LENGTH);
                self.rt("nz.fail", &[msg, len]);
                self.jump(cont);
                self.b.switch(cont);
                // The list of task ids is the runtime's, dead once joined.
                self.rt("nz.arr_release", &[t]);
            }
            mir::StatementKind::CheckFailed { .. } => self.check_failed(),
            mir::StatementKind::StackCheck { .. } => {
                // The runtime measures the thread's stack (N49); this only asks it. The locals'
                // slots are laid out before the first statement, so a large frame is known here
                // and asked about whole (Q1-A1-01).
                let frame: u64 = self
                    .b
                    .slots
                    .iter()
                    .map(|s| u64::from(s.size).next_multiple_of(u64::from(s.align.max(1))))
                    .fold(0u64, u64::saturating_add);
                if frame >= mir::LARGE_FRAME {
                    // The function's entry checks the whole frame (`frame_entry`); the body
                    // still checks as a function in a call cycle does.
                    self.frame_at = Some(self.at);
                }
                let past = self.rt_v("nz.stack_check", &[], Class::I32);
                let bad = self.cmp_imm(Cmp::Ne, past, Class::I32, 0);
                self.guard(bad, "N0408", crate::STACK_EXHAUSTED);
            }
        }
    }

    fn assign(&mut self, dst: mir::LocalId, value: &mir::Rvalue) {
        let dt = self.ty(dst);
        if self.array_rvalue(dst, value) {
            return;
        }
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
                    self.clear(*l);
                    self.set(dst, v);
                }
            }
            mir::Rvalue::Neg(o) if dt != Ty::Int => {
                let x = self.value(o);
                let k = super::numeric::kind_of(dt).unwrap_or(nazm_sema::NumKind::I64);
                let r = self.num_neg(k, x);
                self.set(dst, r);
            }
            mir::Rvalue::Neg(o) => {
                let x = self.value(o);
                let z = self.iconst(0);
                let r = self.checked(super::ir::OvfOp::Sub, z, x, "this negation overflows `Int`");
                self.set(dst, r);
            }
            mir::Rvalue::Arith(op, a, b) if dt != Ty::Int => {
                let x = self.value(a);
                let y = self.value(b);
                let k = super::numeric::kind_of(dt).unwrap_or(nazm_sema::NumKind::I64);
                let r = self.num_arith(*op, k, x, y);
                self.set(dst, r);
            }
            mir::Rvalue::Arith(op, a, b) => {
                let x = self.value(a);
                let y = self.value(b);
                let r = self.arith(*op, x, y);
                self.set(dst, r);
            }
            // Gate 2's numeric forms (`super::numeric`).
            mir::Rvalue::Bits(op, a, b) => {
                let x = self.value(a);
                let y = self.value(b);
                let k = super::numeric::kind_of(dt).unwrap_or(nazm_sema::NumKind::I64);
                let r = self.num_bits(*op, k, x, y);
                self.set(dst, r);
            }
            mir::Rvalue::Complement(a) => {
                let x = self.value(a);
                let k = super::numeric::kind_of(dt).unwrap_or(nazm_sema::NumKind::I64);
                let r = self.num_complement(k, x);
                self.set(dst, r);
            }
            mir::Rvalue::Convert { mode, to, operand } => {
                let from = super::numeric::kind_of(self.operand_ty(operand));
                let to = super::numeric::kind_of(Ty::of(*to));
                let x = self.value(operand);
                let r = match (from, to) {
                    (Some(f), Some(t)) => self.num_convert(*mode, f, t, x),
                    _ => {
                        let at = self.at;
                        self.lw.defect(at, "a conversion between non-numbers");
                        x
                    }
                };
                self.set(dst, r);
            }
            // Taken by `array_rvalue` above.
            mir::Rvalue::Array { .. }
            | mir::Rvalue::ArrayRepeat { .. }
            | mir::Rvalue::Index { .. } => {}
            mir::Rvalue::Fits { to, operand } => {
                let from = super::numeric::kind_of(self.operand_ty(operand));
                let to = super::numeric::kind_of(Ty::of(*to));
                let x = self.value(operand);
                let r = match (from, to) {
                    (Some(f), Some(t)) => self.num_fits(f, t, x),
                    _ => self.bit(false),
                };
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
                        self.negate_if(eq, *op == mir::CmpOp::NotEq)
                    }
                    Ty::Record(_) | Ty::Enum(_) | Ty::Array(_) => {
                        let a = self.place(lhs);
                        let b = self.place(rhs);
                        let h = self.lw.helper(Helper::Equal(t));
                        let eq = match self.call(h, vec![a, b]) {
                            Some(v) => v,
                            None => self.bit(false),
                        };
                        self.negate_if(eq, *op == mir::CmpOp::NotEq)
                    }
                    Ty::Num(k) => {
                        let a = self.value(lhs);
                        let b = self.value(rhs);
                        self.num_compare(*op, k, a, b)
                    }
                    _ => {
                        let a = self.value(lhs);
                        let b = self.value(rhs);
                        let cc = match op {
                            mir::CmpOp::Eq => Cmp::Eq,
                            mir::CmpOp::NotEq => Cmp::Ne,
                            mir::CmpOp::Lt => Cmp::Lt,
                            mir::CmpOp::LtEq => Cmp::Le,
                            mir::CmpOp::Gt => Cmp::Gt,
                            mir::CmpOp::GtEq => Cmp::Ge,
                        };
                        self.cmp(cc, a, b)
                    }
                };
                self.set(dst, r);
            }
            mir::Rvalue::Call { callee, args } => self.direct_call(dst, *callee, args),
            mir::Rvalue::Primitive { op, elem, args } => {
                self.primitive(dst, *op, elem.map(Ty::of), args);
            }
            mir::Rvalue::Closure { func, captures } => {
                let Some(fsym) = self.lw.funcs.get(func).copied() else {
                    let at = self.at;
                    self.lw.defect(at, "a closure of an undeclared function");
                    return;
                };
                let carried: Vec<Ty> = self
                    .lw
                    .native
                    .program
                    .function(*func)
                    .and_then(|g| g.closure.as_ref())
                    .map(|c| c.iter().map(|t| Ty::of(*t)).collect())
                    .unwrap_or_default();
                let (offsets, size) = self.closure_layout(&carried);
                let code = self.addr_of(fsym);
                let release = if carried.iter().any(|t| self.lw.lay.owns(*t)) {
                    let h = self.lw.helper(Helper::ClosureRelease(*func));
                    self.addr_of(h)
                } else {
                    self.null()
                };
                let n = self.iconst(i64::from(size));
                let c = self.rt_v("nz.closure_new", &[n, code, release], Class::Ptr);
                let none = self.is_null(c);
                self.guard(none, "N0406", "out of memory");
                for ((o, t), off) in captures.iter().zip(&carried).zip(&offsets) {
                    let at = self.ptr_off(c, i64::from(*off));
                    // The object's own reference to each capture.
                    if in_memory(*t) {
                        let src = self.place(o);
                        let n = self.size(*t);
                        self.copy(at, src, n);
                        self.retain_at(at, *t);
                    } else {
                        let v = self.value(o);
                        self.store(at, 0, v);
                        if self.lw.lay.owns(*t) {
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
                    let at = self.at;
                    self.lw.defect(at, "a capture the closure does not carry");
                    return;
                };
                let c = self.get(*env);
                let at = self.ptr_off(c, i64::from(off));
                self.read_owned(dst, at, t);
            }
            mir::Rvalue::CallIndirect { callee, args } => {
                let c = self.value(callee);
                let fn_ty = self.operand_ty(callee);
                let Ty::Fn(id) = fn_ty else {
                    let at = self.at;
                    self.lw
                        .defect(at, "a call through something not a function value");
                    return;
                };
                let Some(fsig) = self
                    .lw
                    .native
                    .program
                    .types
                    .function(nazm_sema::FnTypeId::from_index(id.0 as usize))
                    .cloned()
                else {
                    let at = self.at;
                    self.lw.defect(at, "a function type with no signature");
                    return;
                };
                let mut params = vec![fn_ty];
                params.extend(fsig.params.iter().map(|t| Ty::of(*t)));
                let sig = Lw::nazm_sig(&params, Ty::of(fsig.ret));
                let code = self.load(Class::Ptr, c, rt::CLOSURE_CODE);
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
                let result = sig.ret.map(|_| self.b.value());
                self.b.push(Inst::CallIndirect {
                    dst: result,
                    addr: code,
                    sig,
                    args: vals,
                });
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
                    let at = self.ptr_off(d, i64::from(off));
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
                self.store(d, 0, t);
                for (field, tmp) in fields {
                    let (off, _) = self.payload_offset(*enumeration, tag, *field);
                    let at = self.ptr_off(d, i64::from(off));
                    self.move_into(at, *tmp);
                }
            }
            mir::Rvalue::Field { base, field } => {
                let b = self.place(base);
                let (off, t) = self.field_offset(*field);
                let at = self.ptr_off(b, i64::from(off));
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
                let at = self.ptr_off(b, i64::from(off));
                self.read_owned(dst, at, t);
            }
        }
    }

    fn negate_if(&mut self, v: V, negate: bool) -> V {
        if negate {
            let one = self.bit(true);
            self.bin(BinOp::Xor, v, one)
        } else {
            v
        }
    }

    /// A direct call: to a Nazm function, or to C with its conversions (N55, N85).
    fn direct_call(&mut self, dst: mir::LocalId, callee: mir::FuncId, args: &[mir::Operand]) {
        let dt = self.ty(dst);
        let Some(fsym) = self.lw.funcs.get(&callee).copied() else {
            let at = self.at;
            self.lw.defect(at, "a call to an undeclared function");
            return;
        };
        let foreign = self
            .lw
            .native
            .sigs
            .get(callee.index())
            .is_some_and(|s| s.foreign);
        let mut vals = Vec::with_capacity(args.len() + 1);
        if in_memory(dt) && !foreign {
            let d = self.addr(dst);
            vals.push(d);
        }
        // A `Str` argument to C is a NUL-terminated copy C borrows, freed after the call (N55,
        // §7.57); one holding a NUL fails the call before C runs. A C-layout record (N85) is a
        // copy of its bytes — already C's layout, by `abi` — that C borrows for the call.
        let mut copies: Vec<V> = Vec::new();
        for a in args {
            let v = self.argument(a);
            if foreign {
                match self.operand_ty(a) {
                    t @ Ty::Record(_) => {
                        let size = self.iconst(i64::from(self.size(t)));
                        let m = self.rt_v("malloc", &[size], Class::Ptr);
                        let oom = self.is_null(m);
                        self.foreign_guard(oom, "N0406", "out of memory", &copies);
                        self.rt("memcpy", &[m, v, size]);
                        copies.push(m);
                        vals.push(m);
                        continue;
                    }
                    Ty::Str => {
                        let [pf, lf, _] = self.lw.data.str_fields();
                        let p = self.load(Class::Ptr, v, offset(pf));
                        let n = self.load(Class::I64, v, offset(lf));
                        let nul = self.rt_v("nz.cstr_has_nul", &[p, n], Class::Bit);
                        self.foreign_guard(
                            nul,
                            "N0405",
                            "a string holding a NUL byte cannot be passed to C",
                            &copies,
                        );
                        let c = self.rt_v("nz.cstr_borrow", &[p, n], Class::Ptr);
                        let oom = self.is_null(c);
                        self.foreign_guard(oom, "N0406", "out of memory", &copies);
                        copies.push(c);
                        vals.push(c);
                        continue;
                    }
                    _ => {}
                }
            }
            vals.push(v);
        }
        let result = self.call(fsym, vals);
        // What C left in `errno`, before anything else can change it (N85).
        if foreign {
            self.rt("nz.errno_save", &[]);
        }
        for c in &copies {
            self.rt("free", &[*c]);
        }
        // A callee that failed returned a stand-in; this is where that is noticed. A foreign
        // callee cannot fail the Nazm way, so nothing is checked (N42).
        if !foreign {
            self.check_failed();
        }
        if !in_memory(dt) {
            if let Some(v) = result {
                self.set(dst, v);
            }
        } else if foreign {
            let at = self.at;
            self.lw
                .defect(at, "a foreign function returning a value held in memory");
        }
    }

    /// An overflow-checked `Int` operation: the result, or the failure.
    pub(crate) fn checked(&mut self, op: super::ir::OvfOp, x: V, y: V, message: &str) -> V {
        let dst = self.b.value();
        let ovf = self.b.value();
        self.b.push(Inst::Ovf {
            dst,
            ovf,
            op,
            a: x,
            b: y,
        });
        self.guard(ovf, "N0400", message);
        dst
    }

    fn arith(&mut self, op: mir::ArithOp, x: V, y: V) -> V {
        use super::ir::OvfOp;
        match op {
            mir::ArithOp::Add => self.checked(OvfOp::Add, x, y, "this addition overflows `Int`"),
            mir::ArithOp::Sub => self.checked(OvfOp::Sub, x, y, "this subtraction overflows `Int`"),
            mir::ArithOp::Mul => {
                self.checked(OvfOp::Mul, x, y, "this multiplication overflows `Int`")
            }
            mir::ArithOp::Div => {
                let zero = self.cmp_imm(Cmp::Eq, y, Class::I64, 0);
                self.guard(zero, "N0401", "division by zero");
                let special = self.min_over_minus_one(x, y);
                self.guard(special, "N0400", "this division overflows `Int`");
                self.bin(BinOp::SDiv, x, y)
            }
            mir::ArithOp::Rem => {
                let zero = self.cmp_imm(Cmp::Eq, y, Class::I64, 0);
                self.guard(zero, "N0401", "division by zero");
                // `Int::MIN % -1` is 0: the division is made by 1 instead, so it never meets the
                // one pair the machine faults on, and the answer is the same.
                let special = self.min_over_minus_one(x, y);
                let one = self.iconst(1);
                let divisor = self.select(special, one, y);
                self.bin(BinOp::SRem, x, divisor)
            }
        }
    }

    fn min_over_minus_one(&mut self, x: V, y: V) -> V {
        let is_min = self.cmp_imm(Cmp::Eq, x, Class::I64, i64::MIN);
        let is_m1 = self.cmp_imm(Cmp::Eq, y, Class::I64, -1);
        self.bin(BinOp::And, is_min, is_m1)
    }

    /// Whether the strings at two addresses have the same bytes: equal lengths, then `memcmp`
    /// only when they are.
    pub(crate) fn string_equal(&mut self, a: V, b: V) -> V {
        let [pf, lf, _] = self.lw.data.str_fields();
        let la = self.load(Class::I64, a, offset(lf));
        let lb = self.load(Class::I64, b, offset(lf));
        let same = self.cmp(Cmp::Eq, la, lb);
        let out = self.b.var(Class::Bit);
        let no = self.bit(false);
        self.set_var(out, no);
        let compare = self.b.block();
        let join = self.b.block();
        self.branch(same, compare, join);
        self.b.switch(compare);
        let pa = self.load(Class::Ptr, a, offset(pf));
        let pb = self.load(Class::Ptr, b, offset(pf));
        let diff = self.rt_v("memcmp", &[pa, pb, la], Class::I32);
        let eq = self.cmp_imm(Cmp::Eq, diff, Class::I32, 0);
        self.set_var(out, eq);
        self.jump(join);
        self.b.switch(join);
        self.get_var(out)
    }

    fn spawn(&mut self, list: mir::LocalId, callee: mir::FuncId, args: &[mir::Operand]) {
        // The block: the thread handle, the task's failure message and length, then each
        // argument at its alignment.
        let mut at = rt::TASK_ARGUMENTS;
        let mut shape = Vec::with_capacity(args.len());
        for a in args {
            let t = self.operand_ty(a);
            let l = self.lw.lay.lay(t).unwrap_or(crate::abi::Lay::new(8, 8));
            at = at.div_ceil(l.align.max(1)) * l.align.max(1);
            shape.push((t, at));
            at += l.size;
        }
        let size = self.iconst(i64::from(at.max(rt::TASK_ARGUMENTS)));
        let blk = self.rt_v("malloc", &[size], Class::Ptr);
        let null = self.is_null(blk);
        self.guard(null, "N0406", "out of memory");
        let zp = self.null();
        self.store(blk, rt::TASK_MESSAGE, zp);
        let zl = self.iconst(0);
        self.store(blk, rt::TASK_LENGTH, zl);
        // The block takes a reference of its own to each argument; the task gives it back.
        for (a, (t, off)) in args.iter().zip(&shape) {
            let dst = self.ptr_off(blk, i64::from(*off));
            if in_memory(*t) {
                let src = self.place(a);
                let n = self.size(*t);
                self.copy(dst, src, n);
                self.retain_at(dst, *t);
            } else {
                let v = self.value(a);
                if self.lw.lay.owns(*t) {
                    self.retain_value(v, *t);
                }
                self.store(dst, 0, v);
            }
        }
        let tramp = self.lw.trampoline(callee, shape.clone());
        let fa = self.addr_of(tramp);
        let tids = self.get(list);
        let rc = self.rt_v("nz.spawn", &[tids, fa, blk], Class::I32);
        let started = self.cmp_imm(Cmp::Eq, rc, Class::I32, 0);
        let cleanup = self.b.block();
        let after = self.b.block();
        self.branch(started, after, cleanup);
        self.b.switch(cleanup);
        // A task that never started owns nothing: give back what was packed for it.
        for (t, off) in &shape {
            let a = self.ptr_off(blk, i64::from(*off));
            self.release_at(a, *t);
        }
        self.rt("free", &[blk]);
        self.jump(after);
        self.b.switch(after);
        let oom = self.cmp_imm(Cmp::Eq, rc, Class::I32, -1);
        self.guard(oom, "N0406", "out of memory");
        let bad = self.cmp_imm(Cmp::Gt, rc, Class::I32, 0);
        self.guard(bad, "N0404", "this task could not be started");
        // Started (N54).
        self.rt("nz.sched_note_spawn", &[]);
    }

    // --- terminators ---------------------------------------------------------------------

    fn terminator(&mut self, t: &mir::Terminator, ret: Ty) {
        match t {
            mir::Terminator::Goto(b) => {
                let blk = self.block(*b);
                self.jump(blk);
            }
            mir::Terminator::Branch {
                cond,
                then,
                otherwise,
            } => {
                let c = self.value(cond);
                let (a, b) = (self.block(*then), self.block(*otherwise));
                self.branch(c, a, b);
            }
            mir::Terminator::Switch {
                scrutinee,
                enumeration,
                arms,
            } => {
                let a = self.addr(*scrutinee);
                let tag = self.load(Class::I64, a, 0);
                let mut cases = Vec::with_capacity(arms.len());
                for (v, b) in arms {
                    let t = self.tag(*enumeration, *v);
                    let blk = self.block(*b);
                    cases.push((i64::from(t), blk));
                }
                // Every variant has an arm, so the default is unreachable for a value this
                // program built; it is the first arm rather than undefined behaviour, so a
                // compiler mistake is a wrong answer.
                let default = match cases.first() {
                    Some((_, b)) => *b,
                    None => self.b.block(),
                };
                self.b.term(Term::Switch {
                    value: tag,
                    cases,
                    default,
                });
            }
            mir::Terminator::Return(l) => {
                if let Some(out) = self.sret {
                    let src = self.addr(*l);
                    let n = self.size(ret);
                    self.copy(out, src, n);
                    self.b.term(Term::Return(None));
                } else {
                    let v = self.get(*l);
                    self.b.term(Term::Return(Some(v)));
                }
            }
            mir::Terminator::Unwind => {
                // The failure exit: a stand-in the caller never reads, because its failure
                // check leaves first.
                if self.sret.is_some() {
                    self.b.term(Term::Return(None));
                } else {
                    let z = self.iconst_c(class(ret), 0);
                    self.b.term(Term::Return(Some(z)));
                }
            }
            mir::Terminator::Exit(code) => {
                let c = self.value(code);
                let narrow = self.trunc(Class::I32, c);
                self.rt("exit", &[narrow]);
                self.b.term(Term::Unreachable);
            }
        }
    }

    fn block(&mut self, b: mir::BlockId) -> B {
        if let Some(blk) = self.blocks.get(b.index()).copied() {
            blk
        } else {
            let at = self.at;
            self.lw.defect(at, "a jump to a block MIR does not have");
            self.b.block()
        }
    }
}

/// A byte offset as an instruction's offset.
pub(crate) fn offset(o: u32) -> i32 {
    i32::try_from(o).unwrap_or(i32::MAX)
}

impl Lw<'_> {
    /// A function symbol's signature.
    pub(crate) fn sig_of(&self, s: SymId) -> Option<Sig> {
        self.symbol_kind(s).and_then(|k| match k {
            super::ir::SymKind::Func { sig } => Some(sig.clone()),
            super::ir::SymKind::Data { .. } => None,
        })
    }
}

/// Which locals need no variable (N59, §7.61): a temporary MIR lowering made, of type `Int` or
/// `Bool`, assigned exactly once and read only later in the block that assigns it. Everything
/// else keeps its variable — parameters, bindings, and anything another block reads.
fn promotable(f: &mir::Function) -> Vec<bool> {
    let n = f.locals.len();
    let mut defs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut uses: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (b, block) in f.blocks.iter().enumerate() {
        for (s, st) in block.stmts.iter().enumerate() {
            if let mir::StatementKind::Assign { dst, .. } = &st.kind {
                if let Some(d) = defs.get_mut(dst.index()) {
                    d.push((b, s));
                }
            }
            for l in st.kind.mentions() {
                if let Some(u) = uses.get_mut(l.index()) {
                    u.push((b, s));
                }
            }
        }
        for l in block.term.reads() {
            if let Some(u) = uses.get_mut(l.index()) {
                u.push((b, usize::MAX));
            }
        }
    }
    f.locals
        .iter()
        .enumerate()
        .map(|(i, local)| {
            let scalar = matches!(local.ty.value().map(Ty::of), Some(Ty::Int | Ty::Bool));
            let temp = matches!(local.origin, mir::LocalOrigin::Temp) && i >= f.params as usize;
            let (Some(d), Some(u)) = (defs.get(i), uses.get(i)) else {
                return false;
            };
            scalar
                && temp
                && d.len() == 1
                && d.first()
                    .is_some_and(|&(db, ds)| u.iter().all(|&(ub, us)| ub == db && us > ds))
        })
        .collect()
}
