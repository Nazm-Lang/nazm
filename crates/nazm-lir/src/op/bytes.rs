//! `Bytes` and `Text` (Gate 2, `docs/general-purpose.md` §5–§6): each built-in as the
//! instructions and runtime calls it means.
//!
//! Both are `Str`'s triple, `{ptr, len, owner}`. A `Bytes` always views a heap buffer whose
//! owner it counts — `bytes_new` and `bytes_from_str` allocate one, `bytes_slice` shares one —
//! so writing through it writes memory the program allocated. A `Text` is a `Str` every way of
//! making which keeps it UTF-8. A range is checked before anything is read (`N0405`); a scalar
//! boundary or value the text needs is checked after (`N0413`).

use nazm_mir as mir;
use nazm_sema::{Intrinsic, NumKind};

use super::func::Fx;
use super::ir::{BinOp, Class, Cmp, ConvOp, V};
use super::lower::num_class;
use super::numeric::kind_of;
use crate::ir::Ty;

/// The sentence a scalar boundary failure stops the program with, in every implementation.
pub(crate) const NOT_UTF8: &str = "this is not UTF-8";

impl Fx<'_, '_> {
    /// `dst = op(args)` for a `Bytes` or `Text` built-in; `false` for any other.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn bytes_text(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) -> bool {
        match op {
            Intrinsic::BytesNew => {
                let n = self.arg(args, 0);
                let neg = self.cmp_imm(Cmp::Lt, n, Class::I64, 0);
                self.guard(neg, "N0405", "a buffer's length is never negative");
                let room = self.add_imm(n, 1);
                let (owner, buf) = self.string_backing(room);
                self.rt("nz.mem_zero", &[buf, n]);
                self.write_str(dst, buf, n, owner);
            }
            Intrinsic::BytesFromStr | Intrinsic::BytesToStr => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                self.fresh_copy(dst, p, len);
            }
            Intrinsic::BytesLen | Intrinsic::TextLen => {
                let s = self.arg_place(args, 0);
                let (_, n) = self.str_parts(s);
                self.set(dst, n);
            }
            Intrinsic::BytesGet => {
                let b = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let (p, len) = self.str_parts(b);
                self.within(i, 1, len, "this index is outside the `Bytes`");
                let at = self.ptr_add(p, i);
                let v = self.load(Class::I8, at, 0);
                self.set(dst, v);
            }
            Intrinsic::BytesSet => {
                let b = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let v = self.arg(args, 2);
                let (p, len) = self.str_parts(b);
                self.within(i, 1, len, "this index is outside the `Bytes`");
                let at = self.ptr_add(p, i);
                self.store(at, 0, v);
                let zero = self.iconst(0);
                self.set(dst, zero);
            }
            Intrinsic::BytesSlice => {
                let b = self.arg_place(args, 0);
                let from = self.arg(args, 1);
                let to = self.arg(args, 2);
                let (p, len) = self.str_parts(b);
                self.range(from, to, len, "this slice is not inside the `Bytes`");
                let start = self.ptr_add(p, from);
                let n = self.bin(BinOp::Sub, to, from);
                // The view keeps the buffer it was cut from alive: the same owner, retained.
                let owner = self.str_owner(b);
                self.rt("nz.str_retain", &[owner]);
                self.write_str(dst, start, n, owner);
            }
            Intrinsic::BytesCopy => {
                let d = self.arg_place(args, 0);
                let s = self.arg_place(args, 1);
                let (pd, ld) = self.str_parts(d);
                let (ps, ls) = self.str_parts(s);
                let longer = self.cmp(Cmp::Gt, ls, ld);
                self.guard(longer, "N0405", "the source is longer than the destination");
                // Two views of one buffer may overlap: a move, not a copy.
                self.rt("nz.mem_move", &[pd, ps, ls]);
                self.set(dst, ls);
            }
            Intrinsic::BytesReadLe | Intrinsic::BytesReadBe => {
                let k = elem.and_then(kind_of).unwrap_or(NumKind::I64);
                let w = i64::from(k.bytes());
                let b = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let (p, len) = self.str_parts(b);
                self.within(i, w, len, "this read is outside the `Bytes`");
                let base = self.ptr_add(p, i);
                let mut acc = self.iconst(0);
                for j in 0..w {
                    let byte = self.load(Class::I8, base, i32::try_from(j).unwrap_or(0));
                    let mut wide = self.ext(Class::I64, byte, false);
                    let shift = if op == Intrinsic::BytesReadLe {
                        j
                    } else {
                        w - 1 - j
                    } * 8;
                    if shift > 0 {
                        let s = self.iconst(shift);
                        wide = self.bin(BinOp::Shl, wide, s);
                    }
                    acc = self.bin(BinOp::Or, acc, wide);
                }
                let v = match k {
                    NumKind::F64 => self.conv(ConvOp::Bits, Class::F64, acc),
                    NumKind::F32 => {
                        let narrow = self.trunc(Class::I32, acc);
                        self.conv(ConvOp::Bits, Class::F32, narrow)
                    }
                    other if num_class(other) == Class::I64 => acc,
                    other => self.trunc(num_class(other), acc),
                };
                self.set(dst, v);
            }
            Intrinsic::BytesWriteLe | Intrinsic::BytesWriteBe => {
                let k = elem.and_then(kind_of).unwrap_or(NumKind::I64);
                let w = i64::from(k.bytes());
                let b = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let v = self.arg(args, 2);
                let (p, len) = self.str_parts(b);
                self.within(i, w, len, "this write is outside the `Bytes`");
                let bits = match k {
                    NumKind::F64 => self.conv(ConvOp::Bits, Class::I64, v),
                    NumKind::F32 => {
                        let narrow = self.conv(ConvOp::Bits, Class::I32, v);
                        self.ext(Class::I64, narrow, false)
                    }
                    other if num_class(other) == Class::I64 => v,
                    _ => self.ext(Class::I64, v, false),
                };
                let base = self.ptr_add(p, i);
                for j in 0..w {
                    let shift = if op == Intrinsic::BytesWriteLe {
                        j
                    } else {
                        w - 1 - j
                    } * 8;
                    let part = if shift > 0 {
                        let s = self.iconst(shift);
                        self.bin(BinOp::LShr, bits, s)
                    } else {
                        bits
                    };
                    let byte = self.trunc(Class::I8, part);
                    self.store(base, i32::try_from(j).unwrap_or(0), byte);
                }
                let zero = self.iconst(0);
                self.set(dst, zero);
            }
            Intrinsic::Utf8Valid => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                let ok = self.rt_v("nz.utf8_valid", &[p, len], Class::Bit);
                self.set(dst, ok);
            }
            Intrinsic::TextFromStr => {
                let s = self.arg_place(args, 0);
                let (p, len) = self.str_parts(s);
                let ok = self.rt_v("nz.utf8_valid", &[p, len], Class::Bit);
                let bad = self.cmp_imm(Cmp::Eq, ok, Class::Bit, 0);
                self.guard(bad, "N0413", NOT_UTF8);
                self.share(dst, s);
            }
            // The same bytes, as another reference to them.
            Intrinsic::TextAssume | Intrinsic::TextAsStr => {
                let s = self.arg_place(args, 0);
                self.share(dst, s);
            }
            Intrinsic::TextConcat => {
                self.primitive(dst, Intrinsic::StrConcat, elem, args);
            }
            Intrinsic::TextIsBoundary => {
                let t = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let (p, len) = self.str_parts(t);
                let at = self.boundary(p, len, i);
                self.set(dst, at);
            }
            Intrinsic::TextSlice => {
                let t = self.arg_place(args, 0);
                let from = self.arg(args, 1);
                let to = self.arg(args, 2);
                let (p, len) = self.str_parts(t);
                self.range(from, to, len, "this slice is not inside the `Text`");
                let a = self.boundary(p, len, from);
                let z = self.boundary(p, len, to);
                let both = self.both(a, z);
                let bad = self.cmp_imm(Cmp::Eq, both, Class::Bit, 0);
                self.guard(bad, "N0413", NOT_UTF8);
                self.primitive(dst, Intrinsic::StrSlice, elem, args);
            }
            Intrinsic::TextScalarAt | Intrinsic::TextNext => {
                let t = self.arg_place(args, 0);
                let i = self.arg(args, 1);
                let (p, len) = self.str_parts(t);
                self.within(i, 1, len, "this index is outside the `Text`");
                let at = self.boundary(p, len, i);
                let bad = self.cmp_imm(Cmp::Eq, at, Class::Bit, 0);
                self.guard(bad, "N0413", NOT_UTF8);
                let v = if op == Intrinsic::TextScalarAt {
                    self.rt_v("nz.utf8_decode", &[p, len, i], Class::I64)
                } else {
                    let lead = self.ptr_add(p, i);
                    let byte = self.load(Class::I8, lead, 0);
                    let b = self.ext(Class::I64, byte, false);
                    let (one, two, three, four) = (
                        self.iconst(1),
                        self.iconst(2),
                        self.iconst(3),
                        self.iconst(4),
                    );
                    let below_f0 = self.cmp_imm(Cmp::Lt, b, Class::I64, 0xF0);
                    let w3 = self.select(below_f0, three, four);
                    let below_e0 = self.cmp_imm(Cmp::Lt, b, Class::I64, 0xE0);
                    let w2 = self.select(below_e0, two, w3);
                    let ascii = self.cmp_imm(Cmp::Lt, b, Class::I64, 0x80);
                    let w = self.select(ascii, one, w2);
                    self.bin(BinOp::Add, i, w)
                };
                self.set(dst, v);
            }
            Intrinsic::TextScalarCount => {
                let t = self.arg_place(args, 0);
                let (p, len) = self.str_parts(t);
                let n = self.rt_v("nz.utf8_count", &[p, len], Class::I64);
                self.set(dst, n);
            }
            Intrinsic::ScalarValid => {
                let c = self.arg(args, 0);
                let ok = self.scalar(c);
                self.set(dst, ok);
            }
            Intrinsic::ScalarText => {
                let c = self.arg(args, 0);
                let ok = self.scalar(c);
                let bad = self.cmp_imm(Cmp::Eq, ok, Class::Bit, 0);
                self.guard(bad, "N0413", "this is not a Unicode scalar value");
                let room = self.iconst(5);
                let (owner, buf) = self.string_backing(room);
                let n = self.rt_v("nz.utf8_encode", &[buf, c], Class::I64);
                self.write_str(dst, buf, n, owner);
            }
            _ => return false,
        }
        true
    }

    /// Stop with `N0405` unless `0 <= i` and `i + width <= len`, written so a large `i` cannot
    /// wrap past the check.
    pub(crate) fn within(&mut self, i: V, width: i64, len: V, message: &'static str) {
        let neg = self.cmp_imm(Cmp::Lt, i, Class::I64, 0);
        let last = self.add_imm(len, -width);
        let past = self.cmp(Cmp::Gt, i, last);
        let bad = self.or(neg, past);
        self.guard(bad, "N0405", message);
    }

    /// Stop with `N0405` unless `0 <= from <= to <= len`.
    fn range(&mut self, from: V, to: V, len: V, message: &'static str) {
        let neg = self.cmp_imm(Cmp::Lt, from, Class::I64, 0);
        let inverted = self.cmp(Cmp::Lt, to, from);
        let past = self.cmp(Cmp::Gt, to, len);
        let a = self.or(neg, inverted);
        let bad = self.or(a, past);
        self.guard(bad, "N0405", message);
    }

    /// `a && b`, on bits.
    fn both(&mut self, a: V, b: V) -> V {
        let no = self.bit(false);
        self.select(a, b, no)
    }

    /// Whether byte index `i` of UTF-8 bytes `p[..len]` begins a scalar value or ends the
    /// text: `i == len`, or `0 <= i < len` and the byte there is not a continuation byte.
    /// Nothing is read outside the bytes.
    fn boundary(&mut self, p: V, len: V, i: V) -> V {
        let r = self.b.var(Class::Bit);
        let end = self.cmp(Cmp::Eq, i, len);
        self.set_var(r, end);
        let nonneg = self.cmp_imm(Cmp::Ge, i, Class::I64, 0);
        let below = self.cmp(Cmp::Lt, i, len);
        let inside = self.both(nonneg, below);
        let look = self.b.block();
        let done = self.b.block();
        self.branch(inside, look, done);
        self.b.switch(look);
        let at = self.ptr_add(p, i);
        let byte = self.load(Class::I8, at, 0);
        let b = self.ext(Class::I64, byte, false);
        let mask = self.iconst(0xC0);
        let top = self.bin(BinOp::And, b, mask);
        let lead = self.cmp_imm(Cmp::Ne, top, Class::I64, 0x80);
        self.set_var(r, lead);
        self.jump(done);
        self.b.switch(done);
        self.get_var(r)
    }

    /// Whether `c` is a Unicode scalar value: `0..=0x10FFFF` and not a surrogate.
    fn scalar(&mut self, c: V) -> V {
        let nonneg = self.cmp_imm(Cmp::Ge, c, Class::I64, 0);
        let max = self.cmp_imm(Cmp::Le, c, Class::I64, 0x10_FFFF);
        let in_range = self.both(nonneg, max);
        let lo = self.cmp_imm(Cmp::Ge, c, Class::I64, 0xD800);
        let hi = self.cmp_imm(Cmp::Le, c, Class::I64, 0xDFFF);
        let surrogate = self.both(lo, hi);
        let (yes, no) = (self.bit(true), self.bit(false));
        let not_surrogate = self.select(surrogate, no, yes);
        self.both(in_range, not_surrogate)
    }

    /// `dst` = the string at `s`, as one more reference to its bytes.
    fn share(&mut self, dst: mir::LocalId, s: V) {
        let d = self.addr(dst);
        let n = self.size(Ty::Str);
        self.copy(d, s, n);
        self.retain_at(d, Ty::Str);
    }

    /// `dst` = a new buffer holding a copy of `p[..len]`.
    pub(crate) fn fresh_copy(&mut self, dst: mir::LocalId, p: V, len: V) {
        // One byte more than needed, so an empty result still owns storage.
        let room = self.add_imm(len, 1);
        let (owner, buf) = self.string_backing(room);
        self.rt("memcpy", &[buf, p, len]);
        self.write_str(dst, buf, len, owner);
    }
}
