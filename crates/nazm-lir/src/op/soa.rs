//! `--layout soa` (N68, `docs/architecture.md` §7.70): a `Vec[R]` whose record's every field is an
//! `Int` or a `Bool` laid out one array per field, `data + (k · cap + i) · 8` for field `k` of
//! element `i`. Decided here for both backends; until N105 it was the LLVM backend's alone.

use nazm_mir as mir;
use nazm_sema::Intrinsic;

use super::func::{offset, Fx};
use super::ir::{BinOp, Class, Cmp, V};
use super::lower::{class, rt};
use crate::ir::{RecordTy, Ty};

impl Fx<'_, '_> {
    /// The fields of a `Vec`'s element, if that `Vec` is laid out field by field.
    pub(crate) fn soa_fields(&self, vec: Ty) -> Option<Vec<Ty>> {
        let Ty::Vec(id) = vec else { return None };
        let elem = self.lw.unit.vecs.iter().find(|l| l.id == id)?.elem;
        self.soa_elem_fields(elem)
    }

    pub(crate) fn soa_elem_fields(&self, elem: Ty) -> Option<Vec<Ty>> {
        if !self.lw.soa {
            return None;
        }
        let Ty::Record(RecordTy(r)) = elem else {
            return None;
        };
        let l = self.lw.unit.records.iter().find(|l| l.id.0 == r)?;
        (!l.fields.is_empty() && l.fields.iter().all(|f| matches!(f, Ty::Int | Ty::Bool)))
            .then(|| l.fields.clone())
    }

    /// A `Vec[R]` operation over the field-by-field layout; `false` for one that is the same in
    /// both layouts (new, len), or for a `Vec` that is not laid out so.
    pub(crate) fn soa_primitive(
        &mut self,
        dst: mir::LocalId,
        op: Intrinsic,
        elem: Option<Ty>,
        args: &[mir::Operand],
    ) -> bool {
        if !matches!(
            op,
            Intrinsic::VecGet | Intrinsic::VecSet | Intrinsic::VecPush | Intrinsic::VecPop
        ) {
            return false;
        }
        let Some(elem) = elem else { return false };
        let Some(fields) = self.soa_elem_fields(elem) else {
            return false;
        };
        let Ty::Record(RecordTy(rid)) = elem else {
            return false;
        };
        let offsets = self
            .lw
            .lay
            .records
            .get(&rid)
            .map(|s| s.offsets.clone())
            .unwrap_or_default();
        let h = match args.first() {
            Some(a) => self.value(a),
            None => self.null(),
        };
        match op {
            Intrinsic::VecGet => {
                let i = self.arg_value(args, 1);
                self.soa_bounds(h, i);
                let d = self.addr(dst);
                self.soa_gather(h, i, d, &fields, &offsets);
            }
            Intrinsic::VecSet => {
                let i = self.arg_value(args, 1);
                self.soa_bounds(h, i);
                let src = match args.get(2) {
                    Some(a) => self.place(a),
                    None => self.null(),
                };
                self.soa_scatter(h, i, src, &fields, &offsets);
                self.set(dst, i);
            }
            Intrinsic::VecPush => {
                let nf = self.iconst(i64::try_from(fields.len()).unwrap_or(0));
                let data = self.rt_v("nz.soa_grow", &[h, nf], Class::Ptr);
                let null = self.is_null(data);
                self.guard(null, "N0406", "out of memory");
                let len = self.load(Class::I64, h, rt::SEQ_LEN);
                let src = match args.get(1) {
                    Some(a) => self.place(a),
                    None => self.null(),
                };
                self.soa_scatter(h, len, src, &fields, &offsets);
                let grown = self.add_imm(len, 1);
                self.store(h, rt::SEQ_LEN, grown);
                self.set(dst, grown);
            }
            Intrinsic::VecPop => {
                let len = self.load(Class::I64, h, rt::SEQ_LEN);
                let empty = self.cmp_imm(Cmp::Le, len, Class::I64, 0);
                self.guard(empty, "N0405", "this sequence is empty");
                let last = self.add_imm(len, -1);
                let d = self.addr(dst);
                self.soa_gather(h, last, d, &fields, &offsets);
                self.store(h, rt::SEQ_LEN, last);
            }
            _ => return false,
        }
        true
    }

    fn arg_value(&mut self, args: &[mir::Operand], i: usize) -> V {
        match args.get(i) {
            Some(a) => self.value(a),
            None => self.iconst(0),
        }
    }

    /// Where field `k` of element `i` lives.
    fn soa_slot(&mut self, h: V, k: usize, i: V) -> V {
        let cap = self.load(Class::I64, h, rt::SEQ_CAP);
        let data = self.load(Class::Ptr, h, rt::SEQ_DATA);
        let kk = self.iconst(i64::try_from(k).unwrap_or(0));
        let base = self.bin(BinOp::Mul, cap, kk);
        let at = self.bin(BinOp::Add, base, i);
        let eight = self.iconst(8);
        let bytes = self.bin(BinOp::Mul, at, eight);
        self.ptr_add(data, bytes)
    }

    fn soa_gather(&mut self, h: V, i: V, d: V, fields: &[Ty], offsets: &[u32]) {
        for (k, (f, off)) in fields.iter().zip(offsets).enumerate() {
            let p = self.soa_slot(h, k, i);
            let v = self.load(class(*f), p, 0);
            self.store(d, offset(*off), v);
        }
    }

    fn soa_scatter(&mut self, h: V, i: V, src: V, fields: &[Ty], offsets: &[u32]) {
        for (k, (f, off)) in fields.iter().zip(offsets).enumerate() {
            let v = self.load(class(*f), src, offset(*off));
            let p = self.soa_slot(h, k, i);
            self.store(p, 0, v);
        }
    }

    /// The bounds check every indexed operation makes, before any field is touched.
    fn soa_bounds(&mut self, h: V, i: V) {
        let len = self.load(Class::I64, h, rt::SEQ_LEN);
        let neg = self.cmp_imm(Cmp::Lt, i, Class::I64, 0);
        let past = self.cmp(Cmp::Ge, i, len);
        let bad = self.or(neg, past);
        self.guard(bad, "N0405", "this index is outside the sequence");
    }
}
