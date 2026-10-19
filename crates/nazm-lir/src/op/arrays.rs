//! Fixed-size arrays (Gate 2, `docs/user/spec.md`, *Arrays*), as instructions.
//!
//! An array is held in memory, its elements contiguous at a stride of the element's size. It
//! holds values — numbers, `Bool`s, strings since R1a (L8), and records and enums of them — so
//! building and copying one copy bytes and take a reference to every string an element holds,
//! and writing one releases the element it replaces. Every index is checked against the length
//! the type states, unsigned, so a negative index is outside too (`N0405`).

use nazm_mir as mir;

use super::func::Fx;
use super::ir::{BinOp, Class, Cmp, V};
use super::lower::{class, in_memory};
use crate::ir::Ty;

/// The sentence an index outside its array stops the program with, in every implementation.
pub(crate) const OUTSIDE: &str = "this index is outside the array";

impl Fx<'_, '_> {
    /// An array type's element and length.
    fn array_of(&mut self, t: Ty) -> Option<(Ty, u32)> {
        match t {
            Ty::Array(crate::ir::ArrayTy(id)) => self.lw.lay.arrays.get(&id).copied(),
            _ => {
                let at = self.at;
                self.lw
                    .defect(at, "an array operation on something that is not an array");
                None
            }
        }
    }

    /// The address of element `i` of the array at `base`.
    fn element_at(&mut self, base: V, i: V, stride: u32) -> V {
        let k = self.iconst(i64::from(stride));
        let off = self.bin(BinOp::Mul, i, k);
        self.ptr_add(base, off)
    }

    /// Stop the program unless `0 <= i < len`.
    fn check_index(&mut self, i: V, len: u32) {
        let outside = self.cmp_imm(Cmp::UGe, i, Class::I64, i64::from(len));
        self.guard(outside, "N0405", OUTSIDE);
    }

    /// Write operand `o`, an element of type `t`, at `at`: its bytes, and a reference of its
    /// own to what it holds — the operand is borrowed (L8).
    fn put_element(&mut self, at: V, o: &mir::Operand, t: Ty) {
        if in_memory(t) {
            let src = self.place(o);
            let n = self.size(t);
            self.copy(at, src, n);
        } else {
            let v = self.value(o);
            self.store(at, 0, v);
        }
        self.retain_at(at, t);
    }

    /// `dst = [elems…]`, `[value; count]` or `base[index]`; `false` for any other rvalue.
    pub(crate) fn array_rvalue(&mut self, dst: mir::LocalId, value: &mir::Rvalue) -> bool {
        match value {
            mir::Rvalue::Array { elems } => {
                let Some((elem, _)) = self.array_of(self.ty(dst)) else {
                    return true;
                };
                let stride = self.size(elem);
                let d = self.addr(dst);
                for (i, o) in elems.iter().enumerate() {
                    let at = self.ptr_off(d, i64::from(stride) * i64::try_from(i).unwrap_or(0));
                    self.put_element(at, o, elem);
                }
                true
            }
            mir::Rvalue::ArrayRepeat { value, count } => {
                let Some((elem, _)) = self.array_of(self.ty(dst)) else {
                    return true;
                };
                let stride = self.size(elem);
                let d = self.addr(dst);
                // The value once, then a loop: a length is up to 65,536 elements.
                let (src, scalar) = if in_memory(elem) {
                    (Some(self.place(value)), None)
                } else {
                    (None, Some(self.value(value)))
                };
                let i = self.b.var(Class::I64);
                let zero = self.iconst(0);
                self.set_var(i, zero);
                let head = self.b.block();
                let body = self.b.block();
                let done = self.b.block();
                self.jump(head);
                self.b.switch(head);
                let iv = self.get_var(i);
                let more = self.cmp_imm(Cmp::Lt, iv, Class::I64, i64::from(*count));
                self.branch(more, body, done);
                self.b.switch(body);
                let iv = self.get_var(i);
                let at = self.element_at(d, iv, stride);
                match (src, scalar) {
                    (Some(s), _) => self.copy(at, s, stride),
                    (_, Some(v)) => self.store(at, 0, v),
                    _ => {}
                }
                // A reference per copy (L8).
                self.retain_at(at, elem);
                let next = self.add_imm(iv, 1);
                self.set_var(i, next);
                self.jump(head);
                self.b.switch(done);
                true
            }
            mir::Rvalue::Index { base, index } => {
                let bt = self.operand_ty(base);
                let Some((elem, len)) = self.array_of(bt) else {
                    return true;
                };
                let i = self.value(index);
                self.check_index(i, len);
                let stride = self.size(elem);
                let b = self.place(base);
                let at = self.element_at(b, i, stride);
                if in_memory(elem) {
                    let d = self.addr(dst);
                    let n = self.size(elem);
                    self.copy(d, at, n);
                    // The copy out is a new reference to what it holds (L8).
                    self.retain_at(d, elem);
                } else {
                    let v = self.load(class(elem), at, 0);
                    self.set(dst, v);
                }
                true
            }
            _ => false,
        }
    }

    /// `base.path = value` through array elements: each index checked, the old value released
    /// (L8), then the value's bytes moved in.
    pub(crate) fn set_element(
        &mut self,
        base: mir::LocalId,
        path: &[mir::PathRef],
        value: mir::LocalId,
    ) {
        let mut a = self.addr(base);
        let mut t = self.ty(base);
        for step in path {
            match step {
                mir::PathRef::Field(f) => {
                    let (off, ft) = self.field_offset(*f);
                    a = self.ptr_off(a, i64::from(off));
                    t = ft;
                }
                mir::PathRef::Index { index, len } => {
                    let Some((elem, _)) = self.array_of(t) else {
                        return;
                    };
                    let i = self.get(*index);
                    self.check_index(i, *len);
                    let stride = self.size(elem);
                    a = self.element_at(a, i, stride);
                    t = elem;
                }
            }
        }
        self.release_at(a, t);
        self.move_into(a, value);
    }
}
