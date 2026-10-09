//! The built-ins, each as the instructions and runtime calls it means.
//!
//! A bounds or capacity check is a guard whose failure edge is MIR's; an allocation, a retain
//! and a release are calls into the runtime registry; a result is owned by its MIR destination.

use nazm_mir as mir;
use nazm_sema::Intrinsic;

use super::func::{offset, Fx};
use super::ir::{BinOp, Class, Cmp, Inst, V};
use super::lower::{class, in_memory, rt, Helper};
use crate::ir::Ty;

impl Fx<'_, '_> {
    pub(crate) fn arg(&mut self, args: &[mir::Operand], i: usize) -> V {
        match args.get(i) {
            Some(a) => self.value(a),
            None => self.iconst(0),
        }
    }

    pub(crate) fn arg_place(&mut self, args: &[mir::Operand], i: usize) -> V {
        match args.get(i) {
            Some(a) => self.place(a),
            None => self.null(),
        }
    }

    /// `dst = op(args)`.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn primitive(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) {
        if self.soa_primitive(dst, op, elem, args) {
            return;
        }
        if let Some(v) = self.num_primitive(op, elem, args) {
            self.set(dst, v);
            return;
        }
        if self.num_text(dst, op, elem, args) {
            return;
        }
        if self.bytes_text(dst, op, elem, args) {
            return;
        }
        if self.files(dst, op, args) {
            return;
        }
        match op {
            // Gate 2's files and handles, lowered above (`op/files.rs`); reaching here is a defect.
            Intrinsic::OsOpenRead
            | Intrinsic::OsOpenWrite
            | Intrinsic::OsDirOpen
            | Intrinsic::OsStdin
            | Intrinsic::OsStatus
            | Intrinsic::OsRead
            | Intrinsic::OsWrite
            | Intrinsic::OsSeek
            | Intrinsic::OsSync
            | Intrinsic::OsClose
            | Intrinsic::OsDirNext
            | Intrinsic::OsStat
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink
            | Intrinsic::OsRename
            | Intrinsic::OsFamily
            | Intrinsic::OsErrorText => {
                let at = self.at;
                self.lw
                    .defect(at, "a files built-in its lowering did not take");
            }
            // Gate 2's numbers, lowered above; reaching here is a defect.
            Intrinsic::WrappingAdd
            | Intrinsic::WrappingSub
            | Intrinsic::WrappingMul
            | Intrinsic::SaturatingAdd
            | Intrinsic::SaturatingSub
            | Intrinsic::SaturatingMul
            | Intrinsic::CountOnes
            | Intrinsic::LeadingZeros
            | Intrinsic::TrailingZeros
            | Intrinsic::RotateLeft
            | Intrinsic::RotateRight
            | Intrinsic::SwapBytes
            | Intrinsic::ToBigEndian
            | Intrinsic::FromBigEndian
            | Intrinsic::ToLittleEndian
            | Intrinsic::FromLittleEndian
            | Intrinsic::FloatSqrt
            | Intrinsic::FloatAbs
            | Intrinsic::FloatFloor
            | Intrinsic::FloatCeil
            | Intrinsic::FloatTrunc
            | Intrinsic::FloatRound
            | Intrinsic::FloatSin
            | Intrinsic::FloatCos
            | Intrinsic::FloatTan
            | Intrinsic::FloatExp
            | Intrinsic::FloatLn
            | Intrinsic::FloatLog2
            | Intrinsic::FloatLog10
            | Intrinsic::FloatPow
            | Intrinsic::FloatAtan2
            | Intrinsic::FloatMin
            | Intrinsic::FloatMax
            | Intrinsic::FloatIsNan
            | Intrinsic::FloatIsInfinite
            | Intrinsic::FloatIsFinite
            | Intrinsic::NumToStr
            | Intrinsic::Float64ToBits
            | Intrinsic::Float64FromBits
            | Intrinsic::Float32ToBits
            | Intrinsic::Float32FromBits
            | Intrinsic::FloatParses
            | Intrinsic::FloatParseRaw => {
                let at = self.at;
                self.lw
                    .defect(at, "a numeric built-in its lowering did not take");
            }
            // Gate 2's bytes and text, lowered above (`op/bytes.rs`); reaching here is a defect.
            Intrinsic::BytesNew
            | Intrinsic::BytesFromStr
            | Intrinsic::BytesLen
            | Intrinsic::BytesGet
            | Intrinsic::BytesSet
            | Intrinsic::BytesSlice
            | Intrinsic::BytesToStr
            | Intrinsic::BytesCopy
            | Intrinsic::BytesReadLe
            | Intrinsic::BytesReadBe
            | Intrinsic::BytesWriteLe
            | Intrinsic::BytesWriteBe
            | Intrinsic::Utf8Valid
            | Intrinsic::TextAsStr
            | Intrinsic::TextLen
            | Intrinsic::TextConcat
            | Intrinsic::TextIsBoundary
            | Intrinsic::TextSlice
            | Intrinsic::TextScalarAt
            | Intrinsic::TextNext
            | Intrinsic::TextScalarCount
            | Intrinsic::ScalarValid
            | Intrinsic::ScalarText
            | Intrinsic::TextFromStr
            | Intrinsic::TextAssume => {
                let at = self.at;
                self.lw
                    .defect(at, "a bytes or text built-in its lowering did not take");
            }
            Intrinsic::StrLen => {
                let s = self.arg_place(args, 0);
                let (_, n) = self.str_parts(s);
                self.set(dst, n);
            }
            Intrinsic::StrByte => {
                let s = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let (p, len) = self.str_parts(s);
                let neg = self.cmp_imm(Cmp::Lt, i, Class::I64, 0);
                let past = self.cmp(Cmp::Ge, i, len);
                let bad = self.or(neg, past);
                self.guard(bad, "N0405", "this byte index is outside the `Str`");
                let at = self.ptr_add(p, i);
                let byte = self.load(Class::I8, at, 0);
                let wide = self.ext(Class::I64, byte, false);
                self.set(dst, wide);
            }
            // Declassification is the checker's, and nothing at run time (N52): the argument, as
            // a reference of its own.
            Intrinsic::StrVouch => {
                let s = self.arg_place(args, 0);
                let d = self.addr(dst);
                let n = self.size(Ty::Str);
                self.copy(d, s, n);
                self.retain_at(d, Ty::Str);
            }
            Intrinsic::StrSlice => {
                let s = self.arg_place(args, 0);
                let from = self.arg(args, 1);
                let to = self.arg(args, 2);
                let (p, len) = self.str_parts(s);
                let neg = self.cmp_imm(Cmp::Lt, from, Class::I64, 0);
                let inverted = self.cmp(Cmp::Lt, to, from);
                let past = self.cmp(Cmp::Gt, to, len);
                let a = self.or(neg, inverted);
                let bad = self.or(a, past);
                self.guard(bad, "N0405", "this slice is not inside the `Str`");
                let start = self.ptr_add(p, from);
                let n = self.bin(BinOp::Sub, to, from);
                // The view keeps the allocation it was cut from alive: the same owner, retained.
                let owner = self.str_owner(s);
                self.rt("nz.str_retain", &[owner]);
                self.write_str(dst, start, n, owner);
            }
            Intrinsic::StrConcat => {
                let a = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (pa, la) = self.str_parts(a);
                let (pb, lb) = self.str_parts(b);
                let n = self.bin(BinOp::Add, la, lb);
                // One byte more than needed, so an empty result still owns storage.
                let room = self.add_imm(n, 1);
                let (owner, buf) = self.string_backing(room);
                self.rt("memcpy", &[buf, pa, la]);
                let rest = self.ptr_add(buf, la);
                self.rt("memcpy", &[rest, pb, lb]);
                self.write_str(dst, buf, n, owner);
            }
            Intrinsic::IntToStr => {
                let v = self.arg(args, 0);
                let room = self.iconst(21);
                let (owner, buf) = self.string_backing(room);
                let n = self.rt_v("nz.format_int", &[buf, v], Class::I64);
                self.write_str(dst, buf, n, owner);
            }
            Intrinsic::StrFromByte => {
                let b = self.arg(args, 0);
                let low = self.cmp_imm(Cmp::Lt, b, Class::I64, 0);
                let high = self.cmp_imm(Cmp::Gt, b, Class::I64, 255);
                let bad = self.or(low, high);
                self.guard(bad, "N0405", "this is not a byte value");
                let room = self.iconst(1);
                let (owner, buf) = self.string_backing(room);
                let narrow = self.trunc(Class::I8, b);
                self.store(buf, 0, narrow);
                let one = self.iconst(1);
                self.write_str(dst, buf, one, owner);
            }
            Intrinsic::ArgsCount => {
                let argc = self.argc();
                self.set(dst, argc);
            }
            Intrinsic::Arg => {
                let i = self.arg(args, 0);
                let argc = self.argc();
                let neg = self.cmp_imm(Cmp::Lt, i, Class::I64, 0);
                let past = self.cmp(Cmp::Ge, i, argc);
                let bad = self.or(neg, past);
                self.guard(bad, "N0405", "this index is outside the arguments");
                // `argv[0]` is the program's own name and is deliberately not visible.
                let d = self.lw.runtime_data("nz.argv", Class::Ptr);
                let at = self.addr_of(d);
                let base = self.load(Class::Ptr, at, 0);
                let shifted = self.add_imm(i, 1);
                let width = self.iconst(i64::from(self.lw.data.pointer));
                let off = self.bin(BinOp::Mul, shifted, width);
                let slot = self.ptr_add(base, off);
                let text = self.load(Class::Ptr, slot, 0);
                let len = self.rt_v("strlen", &[text], Class::I64);
                // The process's own bytes, outliving every value: a null owner.
                let null = self.null();
                self.write_str(dst, text, len, null);
            }
            Intrinsic::Print | Intrinsic::EPrint => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                let fd = self.iconst_c(Class::I32, if op == Intrinsic::Print { 1 } else { 2 });
                let w = self.rt_v("write", &[fd, p, len], Class::I64);
                self.set(dst, w);
            }
            Intrinsic::ExitWith => {
                let at = self.at;
                self.lw
                    .defect(at, "`exit_with` as a value; MIR makes it a terminator");
            }
            Intrinsic::ReadFile => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                let d = self.addr(dst);
                self.rt("nz.read_file_to", &[d, p, len]);
                let [pf, _, _] = self.lw.data.str_fields();
                let got = self.load(Class::Ptr, d, offset(pf));
                let bad = self.is_null(got);
                self.guard(bad, "N0407", "this file could not be read");
            }
            Intrinsic::WriteFile => {
                let a = self.arg_place(args, 0);
                let b = self.arg_place(args, 1);
                let (pp, pn) = self.str_parts(a);
                let (dp, dn) = self.str_parts(b);
                let n = self.rt_v("nz.write_file", &[pp, pn, dp, dn], Class::I64);
                let bad = self.cmp_imm(Cmp::Lt, n, Class::I64, 0);
                self.guard(bad, "N0407", "this file could not be written");
                self.set(dst, n);
            }
            Intrinsic::FileExists => {
                let a = self.arg_place(args, 0);
                let (pp, pn) = self.str_parts(a);
                let yes = self.rt_v("nz.file_exists", &[pp, pn], Class::Bit);
                self.set(dst, yes);
            }
            Intrinsic::IntsNew | Intrinsic::StrsNew | Intrinsic::VecNew => {
                let h = self.rt_v("nz.seq_new", &[], Class::Ptr);
                let null = self.is_null(h);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, h);
            }
            Intrinsic::IntsLen | Intrinsic::StrsLen | Intrinsic::VecLen => {
                let h = self.arg(args, 0);
                let n = self.load(Class::I64, h, rt::SEQ_LEN);
                self.set(dst, n);
            }
            Intrinsic::IntsGet | Intrinsic::StrsGet | Intrinsic::VecGet => {
                let t = element(op, elem);
                let h = self.arg(args, 0);
                let i = self.arg(args, 1);
                let slot = self.sequence_slot(h, i, t);
                // Its own copy, by the element's law, before anything else can happen to the
                // slot.
                self.read_element(dst, slot, t);
            }
            Intrinsic::IntsSet | Intrinsic::StrsSet | Intrinsic::VecSet => {
                let t = element(op, elem);
                let h = self.arg(args, 0);
                let i = self.arg(args, 1);
                let slot = self.sequence_slot(h, i, t);
                // The new value's reference is taken before the old one is released.
                self.store_element(slot, args.get(2), t, true);
                self.set(dst, i);
            }
            Intrinsic::IntsPush | Intrinsic::StrsPush | Intrinsic::VecPush => {
                let t = element(op, elem);
                let h = self.arg(args, 0);
                let stride = self.iconst(i64::from(self.size(t)));
                // Growth first, and nothing taken until it has succeeded.
                let data = self.rt_v("nz.arr_grow", &[h, stride], Class::Ptr);
                let null = self.is_null(data);
                self.guard(null, "N0406", "out of memory");
                let len = self.load(Class::I64, h, rt::SEQ_LEN);
                let off = self.bin(BinOp::Mul, len, stride);
                let slot = self.ptr_add(data, off);
                self.store_element(slot, args.get(1), t, false);
                let grown = self.add_imm(len, 1);
                self.store(h, rt::SEQ_LEN, grown);
                self.set(dst, grown);
            }
            Intrinsic::IntsPop | Intrinsic::StrsPop | Intrinsic::VecPop => {
                let t = element(op, elem);
                let h = self.arg(args, 0);
                let len = self.load(Class::I64, h, rt::SEQ_LEN);
                let empty = self.cmp_imm(Cmp::Le, len, Class::I64, 0);
                self.guard(empty, "N0405", "this sequence is empty");
                let last = self.add_imm(len, -1);
                let data = self.load(Class::Ptr, h, rt::SEQ_DATA);
                let stride = self.iconst(i64::from(self.size(t)));
                let off = self.bin(BinOp::Mul, last, stride);
                let slot = self.ptr_add(data, off);
                // A transfer: the sequence gives up its reference and the destination holds
                // that same one.
                if in_memory(t) {
                    let d = self.addr(dst);
                    let n = self.size(t);
                    self.copy(d, slot, n);
                } else {
                    let v = self.load(class(t), slot, 0);
                    self.set(dst, v);
                }
                self.store(h, rt::SEQ_LEN, last);
            }
            Intrinsic::StrJoin => {
                let h = self.arg(args, 0);
                let sep = self.arg_place(args, 1);
                let (sp, sn) = self.str_parts(sep);
                let d = self.addr(dst);
                self.rt("nz.str_join_to", &[d, h, sp, sn]);
                let [pf, _, _] = self.lw.data.str_fields();
                let got = self.load(Class::Ptr, d, offset(pf));
                let bad = self.is_null(got);
                self.guard(bad, "N0406", "out of memory");
            }
            Intrinsic::ChanNew => {
                let cap = self.arg(args, 0);
                let bad = self.cmp_imm(Cmp::Lt, cap, Class::I64, 1);
                self.guard(bad, "N0405", "a channel needs a capacity of at least 1");
                let c = self.rt_v("nz.chan_new", &[cap], Class::Ptr);
                let null = self.is_null(c);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, c);
            }
            Intrinsic::ChanSend => {
                let c = self.arg(args, 0);
                let v = self.arg(args, 1);
                let ok = self.rt_v("nz.chan_send", &[c, v], Class::Bit);
                self.set(dst, ok);
            }
            Intrinsic::ChanRecv => {
                let c = self.arg(args, 0);
                let buf = self.arg(args, 1);
                // The entry is shared and the position is not: this site's failure text.
                let at = self.at;
                let (m, n) = self.lw.message("N0406", "out of memory", at);
                let p = self.addr_of(m);
                let len = self.iconst(i64::try_from(n).unwrap_or(0));
                let ok = self.rt_v("nz.chan_recv", &[c, buf, p, len], Class::Bit);
                self.check_failed();
                self.set(dst, ok);
            }
            Intrinsic::ChanClose => {
                let c = self.arg(args, 0);
                self.rt("nz.chan_close", &[c]);
                let z = self.iconst(0);
                self.set(dst, z);
            }
            // A generic channel (N53): the runtime copies one slot of the element's size; a send
            // hands over a reference of its own, given back if the channel refuses; a receive
            // takes the slot's reference onto the vector.
            Intrinsic::ChanNewOf => {
                let t = element(op, elem);
                let cap = self.arg(args, 0);
                let bad = self.cmp_imm(Cmp::Lt, cap, Class::I64, 1);
                self.guard(bad, "N0405", "a channel needs a capacity of at least 1");
                let stride = self.iconst(i64::from(self.size(t)));
                let release = if self.lw.lay.owns(t) {
                    let h = self.lw.helper(Helper::ReleaseElement(t));
                    self.addr_of(h)
                } else {
                    self.null()
                };
                let c = self.rt_v("nz.chanv_new", &[cap, stride, release], Class::Ptr);
                let null = self.is_null(c);
                self.guard(null, "N0406", "out of memory");
                self.set(dst, c);
            }
            Intrinsic::ChanSendOf => {
                let t = element(op, elem);
                let c = self.arg(args, 0);
                let slot = self.scratch(self.size(t));
                // The channel's own reference, taken as the value goes into the slot.
                self.store_element(slot, args.get(1), t, false);
                let ok = self.rt_v("nz.chanv_send", &[c, slot], Class::Bit);
                if self.lw.lay.owns(t) {
                    let refused = self.b.block();
                    let cont = self.b.block();
                    self.branch(ok, cont, refused);
                    self.b.switch(refused);
                    self.release_at(slot, t);
                    self.jump(cont);
                    self.b.switch(cont);
                }
                self.set(dst, ok);
            }
            Intrinsic::ChanRecvOf => {
                let t = element(op, elem);
                let c = self.arg(args, 0);
                let h = self.arg(args, 1);
                let slot = self.scratch(self.size(t));
                let got = self.rt_v("nz.chanv_recv", &[c, slot], Class::Bit);
                let take = self.b.block();
                let cont = self.b.block();
                self.branch(got, take, cont);
                self.b.switch(take);
                self.push_moved(h, slot, t);
                self.jump(cont);
                self.b.switch(cont);
                self.set(dst, got);
            }
            Intrinsic::ChanSelectOf | Intrinsic::ChanSelectUntil => {
                // N54: at most one slot copied out, its reference moved onto the vector;
                // `-3 - i` is channel `i` closed and drained.
                let t = element(op, elem);
                let cs = self.arg(args, 0);
                let h = self.arg(args, 1);
                let (ms, timed) = if op == Intrinsic::ChanSelectUntil {
                    (self.arg(args, 2), self.iconst(1))
                } else {
                    (self.iconst(0), self.iconst(0))
                };
                let slot = self.scratch(self.size(t));
                let r = self.rt_v("nz.chanv_select", &[cs, slot, ms, timed], Class::I64);
                let oom = self.cmp_imm(Cmp::Eq, r, Class::I64, i64::MIN);
                self.guard(oom, "N0406", "out of memory");
                let took = self.cmp_imm(Cmp::Ge, r, Class::I64, 0);
                let take = self.b.block();
                let cont = self.b.block();
                self.branch(took, take, cont);
                self.b.switch(take);
                self.push_moved(h, slot, t);
                self.jump(cont);
                self.b.switch(cont);
                let shut = self.cmp_imm(Cmp::Le, r, Class::I64, -3);
                let minus3 = self.iconst(-3);
                let index = self.bin(BinOp::Sub, minus3, r);
                let result = self.select(shut, index, r);
                self.set(dst, result);
            }
            // A contract clause (N87): its kind is the constant second argument.
            Intrinsic::ContractCheck => {
                let cond = self.arg(args, 0);
                let which = match args.get(1) {
                    Some(mir::Operand::Const(mir::Const::Int(k))) => *k,
                    _ => 0,
                };
                let f = self.bit(false);
                let bad = self.cmp(Cmp::Eq, cond, f);
                if which == 0 {
                    self.guard(bad, "N0410", "this precondition does not hold");
                } else {
                    self.guard(bad, "N0411", "this postcondition does not hold");
                }
                let z = self.iconst(0);
                self.set(dst, z);
            }
            // What the last foreign call on this thread left in `errno` (N85).
            Intrinsic::CErrno => {
                let t = self.rt_v("nz.errno_last", &[], Class::I64);
                self.set(dst, t);
            }
            // A pointer C promised was not null (N85): `N0409` before Nazm sees it.
            Intrinsic::ForeignNonNull => {
                let a = self.arg(args, 0);
                // A C pointer, held as a pointer-sized `Int` (N85).
                let null = self.cmp_imm(Cmp::Eq, a, Class::I64, 0);
                self.guard(
                    null,
                    "N0409",
                    "a foreign function returned null where its declaration promised a value",
                );
                self.set(dst, a);
            }
            // A `const char *` C still owns, copied into a string Nazm owns (N85).
            Intrinsic::CStrCopy => {
                let raw = self.arg(args, 0);
                let p = self.int_to_ptr(raw);
                let n = self.rt_v("nz.cstr_len", &[p], Class::I64);
                let room = self.add_imm(n, 1);
                let (owner, buf) = self.string_backing(room);
                self.rt("memcpy", &[buf, p, n]);
                self.write_str(dst, buf, n, owner);
            }
            // An export's C entry as a function pointer (N85), named by the constant symbol Core
            // IR lowering wrote.
            Intrinsic::CallbackAddress => {
                let symbol = match args.first() {
                    Some(mir::Operand::Const(mir::Const::Str(b))) => {
                        String::from_utf8_lossy(b).into_owned()
                    }
                    _ => {
                        let at = self.at;
                        self.lw.defect(at, "a callback address with no symbol");
                        return;
                    }
                };
                let entry = self
                    .lw
                    .native
                    .sigs
                    .iter()
                    .find(|s| s.export.as_ref().is_some_and(|x| x.as_str() == symbol))
                    .map(|s| (s.params.clone(), s.ret));
                let Some((params, ret)) = entry else {
                    let at = self.at;
                    self.lw.defect(at, format!("`{symbol}` is not an export"));
                    return;
                };
                let c = self.lw.c_sig(&params, ret);
                let s = self.lw.symbol(
                    &symbol,
                    super::ir::Linkage::Import,
                    super::ir::SymKind::Func { sig: c },
                );
                let a = self.addr_of(s);
                let word = self.ptr_to_int(a);
                self.set(dst, word);
            }
            Intrinsic::TimeNowMs => {
                let t = self.rt_v("nz.time_now_ms", &[], Class::I64);
                self.set(dst, t);
            }
            // The ordered checked sum (N66), through the runtime, its flag in a slot.
            Intrinsic::IntsSum => {
                let start = self.arg(args, 0);
                let v = self.arg(args, 1);
                let flag = self.scratch(8);
                let z = self.iconst_c(Class::I32, 0);
                self.store(flag, 0, z);
                let t = self.rt_v("nz.ints_sum", &[v, start, flag], Class::I64);
                let raised = self.load(Class::I32, flag, 0);
                let bad = self.cmp_imm(Cmp::Ne, raised, Class::I32, 0);
                self.guard(bad, "N0400", "this addition overflows `Int`");
                self.set(dst, t);
            }
            // A device register (N62), volatile in the runtime.
            Intrinsic::MmioRead32 | Intrinsic::MmioRead8 | Intrinsic::MmioRead16 => {
                let a = self.arg(args, 0);
                let symbol = match op {
                    Intrinsic::MmioRead8 => "nz.mmio_read8",
                    Intrinsic::MmioRead16 => "nz.mmio_read16",
                    _ => "nz.mmio_read32",
                };
                let t = self.rt_v(symbol, &[a], Class::I64);
                self.set(dst, t);
            }
            Intrinsic::MmioWrite32 | Intrinsic::MmioWrite8 | Intrinsic::MmioWrite16 => {
                let a = self.arg(args, 0);
                let v = self.arg(args, 1);
                let symbol = match op {
                    Intrinsic::MmioWrite8 => "nz.mmio_write8",
                    Intrinsic::MmioWrite16 => "nz.mmio_write16",
                    _ => "nz.mmio_write32",
                };
                let t = self.rt_v(symbol, &[a, v], Class::I64);
                self.set(dst, t);
            }
            Intrinsic::ChanCloseOf => {
                let c = self.arg(args, 0);
                // The generic close, which also wakes a select (N54).
                self.rt("nz.chanv_close", &[c]);
                let z = self.iconst(0);
                self.set(dst, z);
            }
        }
    }

    /// Append the element at `slot` to the sequence `h`, moving its reference.
    fn push_moved(&mut self, h: V, slot: V, t: Ty) {
        let stride = self.iconst(i64::from(self.size(t)));
        let data = self.rt_v("nz.arr_grow", &[h, stride], Class::Ptr);
        let null = self.is_null(data);
        self.guard(null, "N0406", "out of memory");
        let len = self.load(Class::I64, h, rt::SEQ_LEN);
        let off = self.bin(BinOp::Mul, len, stride);
        let at = self.ptr_add(data, off);
        // Moved, not copied: the slot's reference becomes the vector's.
        let n = self.size(t);
        self.copy(at, slot, n);
        let grown = self.add_imm(len, 1);
        self.store(h, rt::SEQ_LEN, grown);
    }

    /// A string's pointer and length.
    pub(crate) fn str_parts(&mut self, s: V) -> (V, V) {
        let [pf, lf, _] = self.lw.data.str_fields();
        let p = self.load(Class::Ptr, s, offset(pf));
        let n = self.load(Class::I64, s, offset(lf));
        (p, n)
    }

    pub(crate) fn str_owner(&mut self, s: V) -> V {
        let [_, _, of] = self.lw.data.str_fields();
        self.load(Class::Ptr, s, offset(of))
    }

    pub(crate) fn write_str(&mut self, dst: mir::LocalId, p: V, n: V, owner: V) {
        let d = self.addr(dst);
        let [pf, lf, of] = self.lw.data.str_fields();
        self.store(d, offset(pf), p);
        self.store(d, offset(lf), n);
        self.store(d, offset(of), owner);
    }

    /// A string backing of `bytes` usable bytes, as `(owner, data)`.
    pub(crate) fn string_backing(&mut self, bytes: V) -> (V, V) {
        let owner = self.rt_v("nz.str_alloc", &[bytes], Class::Ptr);
        let null = self.is_null(owner);
        self.guard(null, "N0406", "out of memory");
        let data = self.ptr_off(owner, rt::STRING_HEADER);
        (owner, data)
    }

    fn argc(&mut self) -> V {
        let d = self.lw.runtime_data("nz.argc", Class::I64);
        let at = self.addr_of(d);
        self.load(Class::I64, at, 0)
    }

    /// The address of element `i`, bounds-checked.
    fn sequence_slot(&mut self, h: V, i: V, t: Ty) -> V {
        let len = self.load(Class::I64, h, rt::SEQ_LEN);
        let neg = self.cmp_imm(Cmp::Lt, i, Class::I64, 0);
        let past = self.cmp(Cmp::Ge, i, len);
        let bad = self.or(neg, past);
        self.guard(bad, "N0405", "this index is outside the sequence");
        let data = self.load(Class::Ptr, h, rt::SEQ_DATA);
        let stride = self.iconst(i64::from(self.size(t)));
        let off = self.bin(BinOp::Mul, i, stride);
        self.ptr_add(data, off)
    }

    fn read_element(&mut self, dst: mir::LocalId, slot: V, t: Ty) {
        if in_memory(t) {
            let d = self.addr(dst);
            let n = self.size(t);
            self.copy(d, slot, n);
            self.retain_at(d, t);
        } else {
            let v = self.load(class(t), slot, 0);
            if self.lw.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.set(dst, v);
        }
    }

    /// Store a borrowed operand into an element slot, taking a reference of the container's
    /// own; with `replace`, the old element is released after.
    fn store_element(&mut self, slot: V, value: Option<&mir::Operand>, t: Ty, replace: bool) {
        let Some(value) = value else {
            let at = self.at;
            self.lw.defect(at, "a sequence write with no value");
            return;
        };
        if in_memory(t) {
            let n = self.size(t);
            let src = self.place(value);
            let old = if replace {
                // Kept aside so that a value sharing the old one's storage is safe.
                let o = self.scratch(n);
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
            let old = replace.then(|| self.load(class(t), slot, 0));
            if self.lw.lay.owns(t) {
                self.retain_value(v, t);
            }
            self.store(slot, 0, v);
            if let Some(o) = old {
                if self.lw.lay.owns(t) {
                    self.release_value(o, t);
                }
            }
        }
    }

    /// Unused unless a backend needs a raw instruction here; kept so the builder's surface is
    /// whole.
    #[allow(dead_code)]
    fn raw(&mut self, i: Inst) {
        self.b.push(i);
    }
}

/// The element type of a sequence built-in.
pub(crate) fn element(op: Intrinsic, elem: Option<Ty>) -> Ty {
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

impl Fx<'_, '_> {
    /// `num_to_str`, `float_parses` and `float_parse_raw` (Gate 2): the runtime's numbers part.
    fn num_text(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) -> bool {
        use nazm_sema::NumKind;
        match op {
            Intrinsic::NumToStr => {
                let k = elem
                    .and_then(super::numeric::kind_of)
                    .unwrap_or(NumKind::I64);
                let x = self.arg(args, 0);
                let room = self.iconst(48);
                let (owner, buf) = self.string_backing(room);
                let n = if k.is_float() {
                    let d = if k == NumKind::F32 {
                        self.conv(super::ir::ConvOp::FExt, Class::F64, x)
                    } else {
                        x
                    };
                    let single = self.bit(k == NumKind::F32);
                    self.rt_v("nz.format_float", &[buf, d, single], Class::I64)
                } else if k == NumKind::U64 {
                    self.rt_v("nz.format_u64", &[buf, x], Class::I64)
                } else {
                    let wide = if super::lower::num_class(k) == Class::I64 {
                        x
                    } else {
                        self.ext(Class::I64, x, k.is_signed())
                    };
                    self.rt_v("nz.format_int", &[buf, wide], Class::I64)
                };
                self.write_str(dst, buf, n, owner);
                true
            }
            Intrinsic::FloatParses | Intrinsic::FloatParseRaw => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                let r = if op == Intrinsic::FloatParses {
                    self.rt_v("nz.float_parses", &[p, len], Class::Bit)
                } else {
                    self.rt_v("nz.float_parse", &[p, len], Class::F64)
                };
                self.set(dst, r);
                true
            }
            _ => false,
        }
    }
}
