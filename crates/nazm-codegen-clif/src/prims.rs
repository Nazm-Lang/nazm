//! The built-ins, each as the instructions and runtime calls it means.
//!
//! The same checks, failures and ownership as the LLVM backend's (`nazm-lir/src/emit.rs`), in
//! Cranelift IR: a bounds or capacity check is a guard whose failure edge is MIR's; an
//! allocation, a retain and a release are calls into the runtime registry; a result is owned by
//! its MIR destination.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, InstBuilder, Value};
use nazm_lir::Ty;
use nazm_mir as mir;
use nazm_sema::Intrinsic;

use cranelift_module::Module;

use crate::func::{mem, Fx};
use crate::layout::in_memory;

/// A string's header before its bytes: `crate::runtime`'s layout.
const STRING_HEADER: i64 = 16;

impl Fx<'_, '_, '_> {
    /// `dst = op(args)`.
    pub(crate) fn primitive(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) {
        let arg = |fx: &mut Self, i: usize| -> Value {
            match args.get(i) {
                Some(a) => fx.value(a),
                None => fx.iconst(0),
            }
        };
        let place = |fx: &mut Self, i: usize| -> Value {
            match args.get(i) {
                Some(a) => fx.place(a),
                None => fx.iconst(0),
            }
        };
        match op {
            Intrinsic::StrLen => {
                let s = place(self, 0);
                let n = self.b.ins().load(types::I64, mem(), s, 8);
                self.set(dst, n);
            }
            Intrinsic::StrByte => {
                let s = place(self, 0);
                let i = arg(self, 1);
                let (p, len) = self.str_parts(s);
                let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, i, 0);
                let past = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, i, len);
                let bad = self.b.ins().bor(neg, past);
                self.guard(bad, "N0405", "this byte index is outside the `Str`");
                let at = self.b.ins().iadd(p, i);
                let byte = self.b.ins().uload8(types::I64, mem(), at, 0);
                self.set(dst, byte);
            }
            // Declassification is the checker's, and nothing at run time (N52): the argument,
            // as a reference of its own.
            Intrinsic::StrVouch => {
                let s = place(self, 0);
                let d = self.addr(dst);
                let n = self.size(Ty::Str);
                self.copy(d, s, n);
                self.retain_at(d, Ty::Str);
            }
            Intrinsic::StrSlice => {
                let s = place(self, 0);
                let from = arg(self, 1);
                let to = arg(self, 2);
                let (p, len) = self.str_parts(s);
                let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, from, 0);
                let inverted = self.b.ins().icmp(IntCC::SignedLessThan, to, from);
                let past = self.b.ins().icmp(IntCC::SignedGreaterThan, to, len);
                let a = self.b.ins().bor(neg, inverted);
                let bad = self.b.ins().bor(a, past);
                self.guard(bad, "N0405", "this slice is not inside the `Str`");
                let start = self.b.ins().iadd(p, from);
                let n = self.b.ins().isub(to, from);
                // The view keeps the allocation it was cut from alive: the same owner, retained.
                let owner = self.b.ins().load(types::I64, mem(), s, 16);
                self.rt("nz.str_retain", &[owner]);
                self.write_str(dst, start, n, owner);
            }
            Intrinsic::StrConcat => {
                let a = place(self, 0);
                let b = place(self, 1);
                let (pa, la) = self.str_parts(a);
                let (pb, lb) = self.str_parts(b);
                let n = self.b.ins().iadd(la, lb);
                // One byte more than needed, so an empty result still owns storage.
                let room = self.b.ins().iadd_imm_s(n, 1);
                let (owner, buf) = self.string_backing(room);
                self.rt("memcpy", &[buf, pa, la]);
                let rest = self.b.ins().iadd(buf, la);
                self.rt("memcpy", &[rest, pb, lb]);
                self.write_str(dst, buf, n, owner);
            }
            Intrinsic::IntToStr => {
                let v = arg(self, 0);
                let room = self.iconst(21);
                let (owner, buf) = self.string_backing(room);
                let n = self
                    .rt("nz.format_int", &[buf, v])
                    .unwrap_or_else(|| self.iconst(0));
                self.write_str(dst, buf, n, owner);
            }
            Intrinsic::StrFromByte => {
                let b = arg(self, 0);
                let low = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, b, 0);
                let high = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, b, 255);
                let bad = self.b.ins().bor(low, high);
                self.guard(bad, "N0405", "this is not a byte value");
                let room = self.iconst(1);
                let (owner, buf) = self.string_backing(room);
                let narrow = self.b.ins().ireduce(types::I8, b);
                self.b.ins().store(mem(), narrow, buf, 0);
                let one = self.iconst(1);
                self.write_str(dst, buf, one, owner);
            }
            Intrinsic::ArgsCount => {
                let argc = self.argc();
                self.set(dst, argc);
            }
            Intrinsic::Arg => {
                let i = arg(self, 0);
                let argc = self.argc();
                let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, i, 0);
                let past = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, i, argc);
                let bad = self.b.ins().bor(neg, past);
                self.guard(bad, "N0405", "this index is outside the arguments");
                // `argv[0]` is the program's own name and is deliberately not visible.
                let d = self.g.data_import("nz.argv");
                let gv = self.g.module.declare_data_in_func(d, self.b.func);
                let at = self.b.ins().symbol_value(self.g.ptr, gv);
                let base = self.b.ins().load(types::I64, mem(), at, 0);
                let shifted = self.b.ins().iadd_imm_s(i, 1);
                let off = self.b.ins().imul_imm_s(shifted, 8);
                let slot = self.b.ins().iadd(base, off);
                let text = self.b.ins().load(types::I64, mem(), slot, 0);
                let len = self.rt("strlen", &[text]).unwrap_or_else(|| self.iconst(0));
                // The process's own bytes, outliving every value: a null owner.
                let null = self.iconst(0);
                self.write_str(dst, text, len, null);
            }
            Intrinsic::Print | Intrinsic::EPrint => {
                let s = place(self, 0);
                let (p, len) = self.str_parts(s);
                let fd = self
                    .b
                    .ins()
                    .iconst(types::I32, if op == Intrinsic::Print { 1 } else { 2 });
                let w = self
                    .rt("write", &[fd, p, len])
                    .unwrap_or_else(|| self.iconst(0));
                self.set(dst, w);
            }
            Intrinsic::ExitWith => {
                self.g
                    .defect(self.at, "`exit_with` as a value; MIR makes it a terminator");
            }
            Intrinsic::ReadFile => {
                let s = place(self, 0);
                let (p, len) = self.str_parts(s);
                let d = self.addr(dst);
                self.rt("nz.read_file_to", &[d, p, len]);
                let got = self.b.ins().load(types::I64, mem(), d, 0);
                let bad = self.b.ins().icmp_imm_s(IntCC::Equal, got, 0);
                self.guard(bad, "N0407", "this file could not be read");
            }
            Intrinsic::WriteFile => {
                let a = place(self, 0);
                let b = place(self, 1);
                let (pp, pn) = self.str_parts(a);
                let (dp, dn) = self.str_parts(b);
                let n = self
                    .rt("nz.write_file", &[pp, pn, dp, dn])
                    .unwrap_or_else(|| self.iconst(0));
                let bad = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, n, 0);
                self.guard(bad, "N0407", "this file could not be written");
                self.set(dst, n);
            }
            Intrinsic::FileExists => {
                let a = place(self, 0);
                let (pp, pn) = self.str_parts(a);
                let yes = self
                    .rt("nz.file_exists", &[pp, pn])
                    .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                let bit = self.b.ins().band_imm_s(yes, 1);
                self.set(dst, bit);
            }
            Intrinsic::IntsNew | Intrinsic::StrsNew | Intrinsic::VecNew => {
                let h = self.rt("nz.seq_new", &[]).unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, h, 0);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, h);
            }
            Intrinsic::IntsLen | Intrinsic::StrsLen | Intrinsic::VecLen => {
                let h = arg(self, 0);
                let n = self.b.ins().load(types::I64, mem(), h, 0);
                self.set(dst, n);
            }
            Intrinsic::IntsGet | Intrinsic::StrsGet | Intrinsic::VecGet => {
                let t = element(op, elem);
                let h = arg(self, 0);
                let i = arg(self, 1);
                let slot = self.sequence_slot(h, i, t);
                // Its own copy, by the element's law, before anything else can happen to the
                // slot.
                self.read_element(dst, slot, t);
            }
            Intrinsic::IntsSet | Intrinsic::StrsSet | Intrinsic::VecSet => {
                let t = element(op, elem);
                let h = arg(self, 0);
                let i = arg(self, 1);
                let slot = self.sequence_slot(h, i, t);
                // The new value's reference is taken before the old one is released.
                self.store_element(slot, args.get(2), t, true);
                self.set(dst, i);
            }
            Intrinsic::IntsPush | Intrinsic::StrsPush | Intrinsic::VecPush => {
                let t = element(op, elem);
                let h = arg(self, 0);
                let stride = self.iconst(i64::from(self.size(t)));
                // Growth first, and nothing taken until it has succeeded.
                let data = self
                    .rt("nz.arr_grow", &[h, stride])
                    .unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, data, 0);
                self.guard(null, "N0406", "out of memory");
                let len = self.b.ins().load(types::I64, mem(), h, 0);
                let off = self.b.ins().imul(len, stride);
                let slot = self.b.ins().iadd(data, off);
                self.store_element(slot, args.get(1), t, false);
                let grown = self.b.ins().iadd_imm_s(len, 1);
                self.b.ins().store(mem(), grown, h, 0);
                self.set(dst, grown);
            }
            Intrinsic::IntsPop | Intrinsic::StrsPop | Intrinsic::VecPop => {
                let t = element(op, elem);
                let h = arg(self, 0);
                let len = self.b.ins().load(types::I64, mem(), h, 0);
                let empty = self
                    .b
                    .ins()
                    .icmp_imm_s(IntCC::SignedLessThanOrEqual, len, 0);
                self.guard(empty, "N0405", "this sequence is empty");
                let last = self.b.ins().iadd_imm_s(len, -1);
                let data = self.b.ins().load(types::I64, mem(), h, 16);
                let stride = i64::from(self.size(t));
                let off = self.b.ins().imul_imm_s(last, stride);
                let slot = self.b.ins().iadd(data, off);
                // A transfer: the sequence gives up its reference and the destination holds
                // that same one.
                if in_memory(t) {
                    let d = self.addr(dst);
                    let n = self.size(t);
                    self.copy(d, slot, n);
                } else {
                    let v = self.b.ins().load(crate::unit::Gen::clif(t), mem(), slot, 0);
                    self.set(dst, v);
                }
                self.b.ins().store(mem(), last, h, 0);
            }
            Intrinsic::StrJoin => {
                let h = arg(self, 0);
                let sep = place(self, 1);
                let (sp, sn) = self.str_parts(sep);
                let d = self.addr(dst);
                self.rt("nz.str_join_to", &[d, h, sp, sn]);
                let got = self.b.ins().load(types::I64, mem(), d, 0);
                let bad = self.b.ins().icmp_imm_s(IntCC::Equal, got, 0);
                self.guard(bad, "N0406", "out of memory");
            }
            Intrinsic::ChanNew => {
                let cap = arg(self, 0);
                let bad = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, cap, 1);
                self.guard(bad, "N0405", "a channel needs a capacity of at least 1");
                let c = self
                    .rt("nz.chan_new", &[cap])
                    .unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, c, 0);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, c);
            }
            Intrinsic::ChanSend => {
                let c = arg(self, 0);
                let v = arg(self, 1);
                let ok = self
                    .rt("nz.chan_send", &[c, v])
                    .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                let bit = self.b.ins().band_imm_s(ok, 1);
                self.set(dst, bit);
            }
            Intrinsic::ChanRecv => {
                self.g.needs.sequences = true;
                let c = arg(self, 0);
                let buf = arg(self, 1);
                // The helper is shared and the position is not: this site's failure text.
                let (m, n) = self.g.message("N0406", "out of memory", self.at);
                let gv = self.g.module.declare_data_in_func(m, self.b.func);
                let p = self.b.ins().symbol_value(self.g.ptr, gv);
                let len = self.iconst(i64::try_from(n).unwrap_or(0));
                let ok = self
                    .rt("nz.chan_recv", &[c, buf, p, len])
                    .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                self.check_failed();
                let bit = self.b.ins().band_imm_s(ok, 1);
                self.set(dst, bit);
            }
            Intrinsic::ChanClose => {
                let c = arg(self, 0);
                self.rt("nz.chan_close", &[c]);
                let z = self.iconst(0);
                self.set(dst, z);
            }
            // A generic channel (N53), as the LLVM backend has it: the runtime copies one slot
            // of the element's size; a send hands over a reference of its own, given back if
            // the channel refuses; a receive takes the slot's reference onto the vector.
            Intrinsic::ChanNewOf => {
                let t = element(op, elem);
                let cap = arg(self, 0);
                let bad = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, cap, 1);
                self.guard(bad, "N0405", "a channel needs a capacity of at least 1");
                let stride = self.iconst(i64::from(self.size(t)));
                let release = if self.g.lay.owns(t) {
                    let h = self.g.helper(crate::unit::Helper::ReleaseElement(t));
                    let r = self.func(h);
                    self.b.ins().func_addr(self.g.ptr, r)
                } else {
                    self.iconst(0)
                };
                let c = self
                    .rt("nz.chanv_new", &[cap, stride, release])
                    .unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, c, 0);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, c);
            }
            Intrinsic::ChanSendOf => {
                let t = element(op, elem);
                let c = arg(self, 0);
                let slot = self.element_slot(t);
                // The channel's own reference, taken as the value goes into the slot.
                self.store_element(slot, args.get(1), t, false);
                let ok = self
                    .rt("nz.chanv_send", &[c, slot])
                    .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                let bit = self.b.ins().band_imm_s(ok, 1);
                if self.g.lay.owns(t) {
                    let refused = self.b.create_block();
                    let cont = self.b.create_block();
                    self.b.ins().brif(bit, cont, &[], refused, &[]);
                    self.b.switch_to_block(refused);
                    self.release_at(slot, t);
                    self.b.ins().jump(cont, &[]);
                    self.b.switch_to_block(cont);
                }
                self.set(dst, bit);
            }
            Intrinsic::ChanRecvOf => {
                let t = element(op, elem);
                self.g.needs.sequences = true;
                let c = arg(self, 0);
                let h = arg(self, 1);
                let slot = self.element_slot(t);
                let got = self
                    .rt("nz.chanv_recv", &[c, slot])
                    .unwrap_or_else(|| self.b.ins().iconst(types::I8, 0));
                let bit = self.b.ins().band_imm_s(got, 1);
                let take = self.b.create_block();
                let cont = self.b.create_block();
                self.b.ins().brif(bit, take, &[], cont, &[]);
                self.b.switch_to_block(take);
                let stride = self.iconst(i64::from(self.size(t)));
                let data = self
                    .rt("nz.arr_grow", &[h, stride])
                    .unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, data, 0);
                self.guard(null, "N0406", "out of memory");
                let len = self.b.ins().load(types::I64, mem(), h, 0);
                let off = self.b.ins().imul(len, stride);
                let at = self.b.ins().iadd(data, off);
                // Moved, not copied: the slot's reference becomes the vector's.
                let n = self.size(t);
                self.copy(at, slot, n);
                let grown = self.b.ins().iadd_imm_s(len, 1);
                self.b.ins().store(mem(), grown, h, 0);
                self.b.ins().jump(cont, &[]);
                self.b.switch_to_block(cont);
                self.set(dst, bit);
            }
            Intrinsic::ChanSelectOf | Intrinsic::ChanSelectUntil => {
                // N54, as the LLVM backend has it: at most one slot copied out, its reference
                // moved onto the vector; `-3 - i` is channel `i` closed and drained.
                let t = element(op, elem);
                self.g.needs.sequences = true;
                let cs = arg(self, 0);
                let h = arg(self, 1);
                let (ms, timed) = if op == Intrinsic::ChanSelectUntil {
                    (arg(self, 2), self.iconst(1))
                } else {
                    (self.iconst(0), self.iconst(0))
                };
                let slot = self.element_slot(t);
                let r = self
                    .rt("nz.chanv_select", &[cs, slot, ms, timed])
                    .unwrap_or_else(|| self.iconst(-1));
                let oom = self.b.ins().icmp_imm_s(IntCC::Equal, r, i64::MIN);
                self.guard(oom, "N0406", "out of memory");
                let took = self
                    .b
                    .ins()
                    .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, r, 0);
                let take = self.b.create_block();
                let cont = self.b.create_block();
                self.b.ins().brif(took, take, &[], cont, &[]);
                self.b.switch_to_block(take);
                let stride = self.iconst(i64::from(self.size(t)));
                let data = self
                    .rt("nz.arr_grow", &[h, stride])
                    .unwrap_or_else(|| self.iconst(0));
                let null = self.b.ins().icmp_imm_s(IntCC::Equal, data, 0);
                self.guard(null, "N0406", "out of memory");
                let len = self.b.ins().load(types::I64, mem(), h, 0);
                let off = self.b.ins().imul(len, stride);
                let at = self.b.ins().iadd(data, off);
                let n = self.size(t);
                self.copy(at, slot, n);
                let grown = self.b.ins().iadd_imm_s(len, 1);
                self.b.ins().store(mem(), grown, h, 0);
                self.b.ins().jump(cont, &[]);
                self.b.switch_to_block(cont);
                let shut = self.b.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, r, -3);
                let minus3 = self.iconst(-3);
                let index = self.b.ins().isub(minus3, r);
                let result = self.b.ins().select(shut, index, r);
                self.set(dst, result);
            }
            Intrinsic::TimeNowMs => {
                let t = self
                    .rt("nz.time_now_ms", &[])
                    .unwrap_or_else(|| self.iconst(0));
                self.set(dst, t);
            }
            Intrinsic::ChanCloseOf => {
                let c = arg(self, 0);
                // The generic close, which also wakes a select (N54).
                self.rt("nz.chanv_close", &[c]);
                let z = self.iconst(0);
                self.set(dst, z);
            }
        }
    }

    /// A scratch slot for one element of type `t`, which a generic channel copies through.
    fn element_slot(&mut self, t: Ty) -> Value {
        let n = self.size(t).max(8);
        let ss = self
            .b
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                n,
                3,
            ));
        self.b.ins().stack_addr(self.g.ptr, ss, 0)
    }

    /// A string's pointer and length.
    pub(crate) fn str_parts(&mut self, s: Value) -> (Value, Value) {
        let p = self.b.ins().load(types::I64, mem(), s, 0);
        let n = self.b.ins().load(types::I64, mem(), s, 8);
        (p, n)
    }

    fn write_str(&mut self, dst: mir::LocalId, p: Value, n: Value, owner: Value) {
        let d = self.addr(dst);
        self.b.ins().store(mem(), p, d, 0);
        self.b.ins().store(mem(), n, d, 8);
        self.b.ins().store(mem(), owner, d, 16);
    }

    /// A string backing of `bytes` usable bytes, as `(owner, data)`.
    fn string_backing(&mut self, bytes: Value) -> (Value, Value) {
        let owner = self
            .rt("nz.str_alloc", &[bytes])
            .unwrap_or_else(|| self.iconst(0));
        let null = self.b.ins().icmp_imm_s(IntCC::Equal, owner, 0);
        self.guard(null, "N0406", "out of memory");
        let data = self.b.ins().iadd_imm_s(owner, STRING_HEADER);
        (owner, data)
    }

    fn argc(&mut self) -> Value {
        let d = self.g.data_import("nz.argc");
        let gv = self.g.module.declare_data_in_func(d, self.b.func);
        let at = self.b.ins().symbol_value(self.g.ptr, gv);
        self.b.ins().load(types::I64, mem(), at, 0)
    }

    /// The address of element `i`, bounds-checked.
    fn sequence_slot(&mut self, h: Value, i: Value, t: Ty) -> Value {
        let len = self.b.ins().load(types::I64, mem(), h, 0);
        let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, i, 0);
        let past = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, i, len);
        let bad = self.b.ins().bor(neg, past);
        self.guard(bad, "N0405", "this index is outside the sequence");
        let data = self.b.ins().load(types::I64, mem(), h, 16);
        let stride = i64::from(self.size(t));
        let off = self.b.ins().imul_imm_s(i, stride);
        self.b.ins().iadd(data, off)
    }

    fn read_element(&mut self, dst: mir::LocalId, slot: Value, t: Ty) {
        if in_memory(t) {
            let d = self.addr(dst);
            let n = self.size(t);
            self.copy(d, slot, n);
            self.retain_at(d, t);
        } else {
            let v = self.b.ins().load(crate::unit::Gen::clif(t), mem(), slot, 0);
            if self.g.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.set(dst, v);
        }
    }

    /// Store a borrowed operand into an element slot, taking a reference of the container's
    /// own; with `replace`, the old element is released after.
    fn store_element(&mut self, slot: Value, value: Option<&mir::Operand>, t: Ty, replace: bool) {
        let Some(value) = value else {
            self.g.defect(self.at, "a sequence write with no value");
            return;
        };
        if in_memory(t) {
            let n = self.size(t);
            let src = self.place(value);
            let old = if replace {
                // Kept aside so that a value sharing the old one's storage is safe.
                let ss = self
                    .b
                    .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                        n.max(1),
                        3,
                    ));
                let o = self.b.ins().stack_addr(self.g.ptr, ss, 0);
                self.copy(o, slot, n);
                Some(o)
            } else {
                None
            };
            self.retain_at(src, t);
            self.copy(slot, src, n);
            if let Some(o) = old {
                self.release_at(o, t);
            }
        } else {
            let v = self.value(value);
            let old = replace.then(|| self.b.ins().load(crate::unit::Gen::clif(t), mem(), slot, 0));
            if self.g.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.b.ins().store(mem(), v, slot, 0);
            if let Some(o) = old {
                if self.g.lay.owns(t) {
                    self.release_value(o, t);
                }
            }
        }
    }
}

/// The element type of a sequence built-in.
fn element(op: Intrinsic, elem: Option<Ty>) -> Ty {
    match op {
        Intrinsic::IntsGet | Intrinsic::IntsSet | Intrinsic::IntsPush | Intrinsic::IntsPop => {
            Ty::Int
        }
        Intrinsic::StrsGet | Intrinsic::StrsSet | Intrinsic::StrsPush | Intrinsic::StrsPop => {
            Ty::Str
        }
        _ => elem.unwrap_or(Ty::Int),
    }
}
