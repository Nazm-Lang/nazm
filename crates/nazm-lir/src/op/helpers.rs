//! The functions a unit's code asks for: helpers, task trampolines, and export C entries.
//!
//! One helper per type per direction — retain, release, equality, and the per-element release
//! the runtime calls when a `Vec` or a generic channel goes — so a record released in forty
//! places is forty calls, and a nested type's helper calls the inner type's: the recursion is in
//! the generated code, not in this compiler.

use nazm_mir as mir;
use nazm_sema::fnv;

use super::func::Fx;
use super::ir::{Class, Cmp, Function, SymId, Term, B, V};
use super::lower::{class, in_memory, rt, Helper, Lw};
use crate::ir::{EnumTy, RecordTy, Ty};

pub(crate) fn define(lw: &mut Lw<'_>, sym: SymId, h: Helper) -> Function {
    let mut fx = Fx::helper(lw);
    let a = fx.param(0);
    // A release helper's second parameter is the queue of the release in progress (R1a).
    if matches!(h, Helper::Release(_) | Helper::ReleaseElement(_)) {
        fx.queue = Some(fx.param(1));
    }
    let name = match h {
        Helper::Retain(t) | Helper::Release(t) => {
            let retain = matches!(h, Helper::Retain(_));
            each_owned(&mut fx, a, t, retain);
            fx.b.term(Term::Return(None));
            format!(
                "{} of {}",
                if retain { "retain" } else { "release" },
                t.name()
            )
        }
        Helper::ClosureRelease(f) => {
            let carried: Vec<Ty> = fx
                .lw
                .native
                .program
                .function(f)
                .and_then(|g| g.closure.as_ref())
                .map(|c| c.iter().map(|t| Ty::of(*t)).collect())
                .unwrap_or_default();
            let (offsets, _) = fx.closure_layout(&carried);
            for (t, off) in carried.iter().zip(offsets) {
                if fx.lw.lay.owns(*t) {
                    let at = fx.ptr_off(a, i64::from(off));
                    fx.release_at(at, *t);
                }
            }
            fx.b.term(Term::Return(None));
            "a closure object's release".to_owned()
        }
        Helper::ReleaseElement(t) => {
            if in_memory(t) {
                fx.release_at(a, t);
            } else {
                let v = fx.load(Class::Ptr, a, 0);
                fx.release_value(v, t);
            }
            fx.b.term(Term::Return(None));
            format!("release of one {}", t.name())
        }
        Helper::Equal(t) => {
            let b = fx.param(1);
            let differ = fx.b.block();
            equal(&mut fx, a, b, t, differ);
            let yes = fx.bit(true);
            fx.b.term(Term::Return(Some(yes)));
            fx.b.switch(differ);
            let no = fx.bit(false);
            fx.b.term(Term::Return(Some(no)));
            format!("equality of {}", t.name())
        }
        Helper::Order(t) => {
            let b = fx.param(1);
            let less = fx.b.block();
            let greater = fx.b.block();
            order(&mut fx, a, b, t, less, greater);
            let zero = fx.iconst(0);
            fx.b.term(Term::Return(Some(zero)));
            fx.b.switch(less);
            let minus = fx.iconst(-1);
            fx.b.term(Term::Return(Some(minus)));
            fx.b.switch(greater);
            let one = fx.iconst(1);
            fx.b.term(Term::Return(Some(one)));
            format!("order of {}", t.name())
        }
        Helper::Hash(t) => {
            let h = hash(&mut fx, a, t);
            fx.b.term(Term::Return(Some(h)));
            format!("hash of {}", t.name())
        }
    };
    fx.b.finish(sym, name, None)
}

/// Retain or release each owning field of the record or enum at `a`.
fn each_owned(fx: &mut Fx<'_, '_>, a: V, t: Ty, retain: bool) {
    match t {
        Ty::Record(RecordTy(id)) => {
            let Some(s) = fx.lw.lay.records.get(&id).cloned() else {
                let at = fx.at;
                fx.lw.defect(at, "a record with no layout");
                return;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                if !fx.lw.lay.owns(*ft) {
                    continue;
                }
                let at = fx.ptr_off(a, i64::from(*off));
                one(fx, at, *ft, retain);
            }
        }
        Ty::Enum(EnumTy(id)) => {
            let Some(e) = fx.lw.lay.enums.get(&id).cloned() else {
                let at = fx.at;
                fx.lw.defect(at, "an enum with no layout");
                return;
            };
            // The active variant's fields, and only those: which fields exist is decided at
            // run time, by the tag.
            let tag = fx.load(Class::I64, a, 0);
            let done = fx.b.block();
            let mut cases = Vec::new();
            let mut arms = Vec::new();
            for (tagno, v) in e.variants.iter().enumerate() {
                if !v.fields.iter().any(|f| fx.lw.lay.owns(*f)) {
                    continue;
                }
                let blk = fx.b.block();
                cases.push((i64::try_from(tagno).unwrap_or(0), blk));
                arms.push((blk, v.clone()));
            }
            fx.b.term(Term::Switch {
                value: tag,
                cases,
                default: done,
            });
            for (blk, v) in arms {
                fx.b.switch(blk);
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    if !fx.lw.lay.owns(*ft) {
                        continue;
                    }
                    let at = fx.ptr_off(a, i64::from(crate::abi::PAYLOAD + *off));
                    one(fx, at, *ft, retain);
                }
                fx.jump(done);
            }
            fx.b.switch(done);
        }
        // Each element, in a loop (R1a, L8): a length is up to 65,536.
        Ty::Array(crate::ir::ArrayTy(id)) => {
            let Some((elem, len)) = fx.lw.lay.arrays.get(&id).copied() else {
                let at = fx.at;
                fx.lw.defect(at, "an array with no layout");
                return;
            };
            if !fx.lw.lay.owns(elem) {
                return;
            }
            let stride = fx.size(elem);
            let count = fx.iconst(i64::from(len));
            let i = fx.b.var(Class::I64);
            let zero = fx.iconst(0);
            fx.set_var(i, zero);
            let head = fx.b.block();
            let body = fx.b.block();
            let done = fx.b.block();
            fx.jump(head);
            fx.b.switch(head);
            let iv = fx.get_var(i);
            let more = fx.cmp(Cmp::Lt, iv, count);
            fx.branch(more, body, done);
            fx.b.switch(body);
            let iv = fx.get_var(i);
            let k = fx.iconst(i64::from(stride));
            let off = fx.bin(super::ir::BinOp::Mul, iv, k);
            let at = fx.ptr_add(a, off);
            one(fx, at, elem, retain);
            let iv = fx.get_var(i);
            let step = fx.add_imm(iv, 1);
            fx.set_var(i, step);
            fx.jump(head);
            fx.b.switch(done);
        }
        _ => one(fx, a, t, retain),
    }
}

fn one(fx: &mut Fx<'_, '_>, at: V, t: Ty, retain: bool) {
    if retain {
        fx.retain_at(at, t);
    } else {
        fx.release_at(at, t);
    }
}

/// Continue if `same` holds; jump to `differ` if not.
fn next(fx: &mut Fx<'_, '_>, same: V, differ: B) {
    let cont = fx.b.block();
    fx.branch(same, cont, differ);
    fx.b.switch(cont);
}

/// Continue if the values of type `t` at `a` and `b` are equal; jump to `differ` if not.
fn equal(fx: &mut Fx<'_, '_>, a: V, b: V, t: Ty, differ: B) {
    match t {
        Ty::Int | Ty::Bool => {
            let x = fx.load(class(t), a, 0);
            let y = fx.load(class(t), b, 0);
            let same = fx.cmp(Cmp::Eq, x, y);
            next(fx, same, differ);
        }
        // A number's equality is its kind's (Gate 2): IEEE 754's for a float, so a NaN field
        // makes two records unequal.
        Ty::Num(k) => {
            let x = fx.load(class(t), a, 0);
            let y = fx.load(class(t), b, 0);
            let same = fx.num_compare(nazm_mir::CmpOp::Eq, k, x, y);
            next(fx, same, differ);
        }
        Ty::Str => {
            let same = fx.string_equal(a, b);
            next(fx, same, differ);
        }
        Ty::Record(RecordTy(id)) => {
            let Some(s) = fx.lw.lay.records.get(&id).cloned() else {
                return;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                let x = fx.ptr_off(a, i64::from(*off));
                let y = fx.ptr_off(b, i64::from(*off));
                field_equal(fx, x, y, *ft, differ);
            }
        }
        Ty::Enum(EnumTy(id)) => {
            let Some(e) = fx.lw.lay.enums.get(&id).cloned() else {
                return;
            };
            let ta = fx.load(Class::I64, a, 0);
            let tb = fx.load(Class::I64, b, 0);
            let same = fx.cmp(Cmp::Eq, ta, tb);
            next(fx, same, differ);
            let equal_blk = fx.b.block();
            let mut cases = Vec::new();
            let mut arms = Vec::new();
            for (tagno, v) in e.variants.iter().enumerate() {
                if v.fields.is_empty() {
                    continue;
                }
                let blk = fx.b.block();
                cases.push((i64::try_from(tagno).unwrap_or(0), blk));
                arms.push((blk, v.clone()));
            }
            fx.b.term(Term::Switch {
                value: ta,
                cases,
                default: equal_blk,
            });
            for (blk, v) in arms {
                fx.b.switch(blk);
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    let x = fx.ptr_off(a, i64::from(crate::abi::PAYLOAD + *off));
                    let y = fx.ptr_off(b, i64::from(crate::abi::PAYLOAD + *off));
                    field_equal(fx, x, y, *ft, differ);
                }
                fx.jump(equal_blk);
            }
            fx.b.switch(equal_blk);
        }
        // Element by element, in a loop: a length is up to 65,536 (Gate 2).
        Ty::Array(crate::ir::ArrayTy(id)) => {
            let Some((elem, len)) = fx.lw.lay.arrays.get(&id).copied() else {
                return;
            };
            let stride = fx.size(elem);
            let i = fx.b.var(Class::I64);
            let zero = fx.iconst(0);
            fx.set_var(i, zero);
            let head = fx.b.block();
            let body = fx.b.block();
            let done = fx.b.block();
            fx.jump(head);
            fx.b.switch(head);
            let iv = fx.get_var(i);
            let more = fx.cmp_imm(Cmp::Lt, iv, Class::I64, i64::from(len));
            fx.branch(more, body, done);
            fx.b.switch(body);
            let iv = fx.get_var(i);
            let k = fx.iconst(i64::from(stride));
            let off = fx.bin(super::ir::BinOp::Mul, iv, k);
            let x = fx.ptr_add(a, off);
            let y = fx.ptr_add(b, off);
            field_equal(fx, x, y, elem, differ);
            let iv = fx.get_var(i);
            let step = fx.add_imm(iv, 1);
            fx.set_var(i, step);
            fx.jump(head);
            fx.b.switch(done);
        }
        // A `Box` or `List` (L6), a `Vec` underneath: one storage is equal to itself; otherwise
        // the lengths, then element by element. Checking refuses `==` on a `Vec` itself.
        Ty::Vec(v) => {
            let elem = fx
                .lw
                .native
                .program
                .types
                .vecs
                .get(&v.0)
                .map(|i| Ty::of(i.elem));
            let Some(elem) = elem else {
                fx.jump(differ);
                let dead = fx.b.block();
                fx.b.switch(dead);
                return;
            };
            let ha = fx.load(Class::Ptr, a, 0);
            let hb = fx.load(Class::Ptr, b, 0);
            let done = fx.b.block();
            let walk = fx.b.block();
            let one = fx.cmp(Cmp::Eq, ha, hb);
            fx.branch(one, done, walk);
            fx.b.switch(walk);
            let la = fx.load(Class::I64, ha, rt::SEQ_LEN);
            let lb = fx.load(Class::I64, hb, rt::SEQ_LEN);
            let same = fx.cmp(Cmp::Eq, la, lb);
            next(fx, same, differ);
            let da = fx.load(Class::Ptr, ha, rt::SEQ_DATA);
            let db = fx.load(Class::Ptr, hb, rt::SEQ_DATA);
            let stride = fx.size(elem);
            let i = fx.b.var(Class::I64);
            let zero = fx.iconst(0);
            fx.set_var(i, zero);
            let head = fx.b.block();
            let body = fx.b.block();
            fx.jump(head);
            fx.b.switch(head);
            let iv = fx.get_var(i);
            let more = fx.cmp(Cmp::Lt, iv, la);
            fx.branch(more, body, done);
            fx.b.switch(body);
            let iv = fx.get_var(i);
            let k = fx.iconst(i64::from(stride));
            let off = fx.bin(super::ir::BinOp::Mul, iv, k);
            let x = fx.ptr_add(da, off);
            let y = fx.ptr_add(db, off);
            field_equal(fx, x, y, elem, differ);
            let iv = fx.get_var(i);
            let step = fx.add_imm(iv, 1);
            fx.set_var(i, step);
            fx.jump(head);
            fx.b.switch(done);
        }
        // No equality exists for a handle; the checker refuses the comparison.
        _ => {
            fx.jump(differ);
            let dead = fx.b.block();
            fx.b.switch(dead);
        }
    }
}

/// Continue if the values of type `t` at `a` and `b` order equal; jump to `less` or `greater`
/// if not (L6, `docs/spec.md`, *Ordering and hashing are derived*). A record's fields and an
/// enum's variants are already laid out in the canonical order, by name, which is the derived
/// order's: a walk of the layout is the walk the specification describes.
fn order(fx: &mut Fx<'_, '_>, a: V, b: V, t: Ty, less: B, greater: B) {
    let split = |fx: &mut Fx<'_, '_>, lt: V, gt_of: &dyn Fn(&mut Fx<'_, '_>) -> V| {
        let not_less = fx.b.block();
        fx.branch(lt, less, not_less);
        fx.b.switch(not_less);
        let gt = gt_of(fx);
        let cont = fx.b.block();
        fx.branch(gt, greater, cont);
        fx.b.switch(cont);
    };
    match t {
        Ty::Int | Ty::Bool => {
            let x = fx.load(class(t), a, 0);
            let y = fx.load(class(t), b, 0);
            // `false` before `true`: unsigned, as a bit is.
            let (l, g) = if t == Ty::Bool {
                (Cmp::ULt, Cmp::UGt)
            } else {
                (Cmp::Lt, Cmp::Gt)
            };
            let lt = fx.cmp(l, x, y);
            split(fx, lt, &|fx| fx.cmp(g, x, y));
        }
        Ty::Num(k) => {
            let x = fx.load(class(t), a, 0);
            let y = fx.load(class(t), b, 0);
            let lt = fx.num_compare(mir::CmpOp::Lt, k, x, y);
            split(fx, lt, &|fx| fx.num_compare(mir::CmpOp::Gt, k, x, y));
        }
        // By bytes: the common prefix, then the shorter first.
        Ty::Str => {
            let [pf, lf, _] = fx.lw.data.str_fields();
            let la = fx.load(Class::I64, a, super::func::offset(lf));
            let lb = fx.load(Class::I64, b, super::func::offset(lf));
            let m = fx.b.var(Class::I64);
            fx.set_var(m, la);
            let shorter = fx.b.block();
            let measured = fx.b.block();
            let b_short = fx.cmp(Cmp::Lt, lb, la);
            fx.branch(b_short, shorter, measured);
            fx.b.switch(shorter);
            fx.set_var(m, lb);
            fx.jump(measured);
            fx.b.switch(measured);
            let n = fx.get_var(m);
            let pa = fx.load(Class::Ptr, a, super::func::offset(pf));
            let pb = fx.load(Class::Ptr, b, super::func::offset(pf));
            let diff = fx.rt_v("memcmp", &[pa, pb, n], Class::I32);
            let lt = fx.cmp_imm(Cmp::Lt, diff, Class::I32, 0);
            split(fx, lt, &|fx| fx.cmp_imm(Cmp::Gt, diff, Class::I32, 0));
            let lt = fx.cmp(Cmp::Lt, la, lb);
            split(fx, lt, &|fx| fx.cmp(Cmp::Gt, la, lb));
        }
        Ty::Record(RecordTy(id)) => {
            let Some(s) = fx.lw.lay.records.get(&id).cloned() else {
                return;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                let x = fx.ptr_off(a, i64::from(*off));
                let y = fx.ptr_off(b, i64::from(*off));
                field_order(fx, x, y, *ft, less, greater);
            }
        }
        Ty::Enum(EnumTy(id)) => {
            let Some(e) = fx.lw.lay.enums.get(&id).cloned() else {
                return;
            };
            // The tag is the canonical position, by name.
            let ta = fx.load(Class::I64, a, 0);
            let tb = fx.load(Class::I64, b, 0);
            let lt = fx.cmp(Cmp::Lt, ta, tb);
            split(fx, lt, &|fx| fx.cmp(Cmp::Gt, ta, tb));
            let tied = fx.b.block();
            let mut cases = Vec::new();
            let mut arms = Vec::new();
            let carrying = e.variants.iter().enumerate();
            for (tagno, v) in carrying.filter(|(_, v)| !v.fields.is_empty()) {
                let blk = fx.b.block();
                cases.push((i64::try_from(tagno).unwrap_or(0), blk));
                arms.push((blk, v.clone()));
            }
            fx.b.term(Term::Switch {
                value: ta,
                cases,
                default: tied,
            });
            for (blk, v) in arms {
                fx.b.switch(blk);
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    let x = fx.ptr_off(a, i64::from(crate::abi::PAYLOAD + *off));
                    let y = fx.ptr_off(b, i64::from(crate::abi::PAYLOAD + *off));
                    field_order(fx, x, y, *ft, less, greater);
                }
                fx.jump(tied);
            }
            fx.b.switch(tied);
        }
        // Nothing else orders; the checker refuses the comparison.
        _ => {}
    }
}

/// A 64-bit constant, by its bits.
#[allow(clippy::cast_possible_wrap)]
fn word_const(fx: &mut Fx<'_, '_>, x: u64) -> V {
    fx.iconst(x as i64)
}

/// `hash_finish` (`nazm_sema::fnv::finish`), as instructions.
fn finish(fx: &mut Fx<'_, '_>, x: V) -> V {
    use super::ir::BinOp::{LShr, Mul, Xor};
    let mut x = x;
    for (shift, mul) in [(30, Some(fnv::MIX1)), (27, Some(fnv::MIX2)), (31, None)] {
        let s = fx.iconst(shift);
        let hi = fx.bin(LShr, x, s);
        x = fx.bin(Xor, x, hi);
        if let Some(m) = mul {
            let m = word_const(fx, m);
            x = fx.bin(Mul, x, m);
        }
    }
    x
}

/// `hash_u64` (`nazm_sema::fnv::word`) of a 64-bit value, as instructions: eight bytes, least
/// significant first, unrolled.
fn word(fx: &mut Fx<'_, '_>, x: V) -> V {
    use super::ir::BinOp::{And, LShr, Mul, Xor};
    let mut h = word_const(fx, fnv::OFFSET);
    let prime = word_const(fx, fnv::PRIME);
    let byte = fx.iconst(255);
    let eight = fx.iconst(8);
    let mut w = x;
    for _ in 0..8 {
        let low = fx.bin(And, w, byte);
        let mixed = fx.bin(Xor, h, low);
        h = fx.bin(Mul, mixed, prime);
        w = fx.bin(LShr, w, eight);
    }
    finish(fx, h)
}

/// `hash_combine` (`nazm_sema::fnv::combine`), as instructions.
fn combine(fx: &mut Fx<'_, '_>, a: V, b: V) -> V {
    use super::ir::BinOp::{Add, Mul};
    let prime = word_const(fx, fnv::PRIME);
    let m = fx.bin(Mul, a, prime);
    let s = fx.bin(Add, m, b);
    finish(fx, s)
}

/// The derived hash of the value of type `t` at `a` (L6, `docs/spec.md`, *Ordering and
/// hashing are derived*): the library's hash of an integer's 64-bit pattern, a `Bool` as 0 or
/// 1 and a `Str`'s bytes; a record from `hash_u64(0)` and an enum value from its variant's name,
/// each component folded in with `hash_combine` in the canonical order — the layout's.
fn hash(fx: &mut Fx<'_, '_>, a: V, t: Ty) -> V {
    use super::ir::BinOp::{Mul, Xor};
    match t {
        Ty::Int => {
            let x = fx.load(Class::I64, a, 0);
            word(fx, x)
        }
        Ty::Bool => {
            let x = fx.load(Class::Bit, a, 0);
            let x = fx.ext(Class::I64, x, false);
            word(fx, x)
        }
        Ty::Num(k) => {
            let x = fx.load(class(t), a, 0);
            let x = if class(t) == Class::I64 {
                x
            } else {
                fx.ext(Class::I64, x, k.is_signed())
            };
            word(fx, x)
        }
        Ty::Str => {
            let [pf, lf, _] = fx.lw.data.str_fields();
            let n = fx.load(Class::I64, a, super::func::offset(lf));
            let p = fx.load(Class::Ptr, a, super::func::offset(pf));
            let h = fx.b.var(Class::I64);
            let start = word_const(fx, fnv::OFFSET);
            fx.set_var(h, start);
            let i = fx.b.var(Class::I64);
            let zero = fx.iconst(0);
            fx.set_var(i, zero);
            let head = fx.b.block();
            let body = fx.b.block();
            let done = fx.b.block();
            fx.jump(head);
            fx.b.switch(head);
            let iv = fx.get_var(i);
            let more = fx.cmp(Cmp::Lt, iv, n);
            fx.branch(more, body, done);
            fx.b.switch(body);
            let iv = fx.get_var(i);
            let at = fx.ptr_add(p, iv);
            let b = fx.load(Class::I8, at, 0);
            let b = fx.ext(Class::I64, b, false);
            let hv = fx.get_var(h);
            let mixed = fx.bin(Xor, hv, b);
            let prime = word_const(fx, fnv::PRIME);
            let next_h = fx.bin(Mul, mixed, prime);
            fx.set_var(h, next_h);
            let step = fx.add_imm(iv, 1);
            fx.set_var(i, step);
            fx.jump(head);
            fx.b.switch(done);
            let hv = fx.get_var(h);
            finish(fx, hv)
        }
        Ty::Record(RecordTy(id)) => {
            let mut h = word_const(fx, fnv::word(0));
            let Some(s) = fx.lw.lay.records.get(&id).cloned() else {
                return h;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                let x = fx.ptr_off(a, i64::from(*off));
                let fh = field_hash(fx, x, *ft);
                h = combine(fx, h, fh);
            }
            h
        }
        Ty::Enum(EnumTy(id)) => {
            let out = fx.b.var(Class::I64);
            let zero = fx.iconst(0);
            fx.set_var(out, zero);
            let (Some(e), Some(info)) = (
                fx.lw.lay.enums.get(&id).cloned(),
                fx.lw.native.program.types.enums.get(&id).cloned(),
            ) else {
                return zero;
            };
            let tag = fx.load(Class::I64, a, 0);
            let done = fx.b.block();
            let mut cases = Vec::new();
            let mut arms = Vec::new();
            for (tagno, v) in e.variants.iter().enumerate() {
                let name = info
                    .canonical
                    .get(tagno)
                    .and_then(|d| info.variants.get(*d as usize))
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                let blk = fx.b.block();
                cases.push((i64::try_from(tagno).unwrap_or(0), blk));
                arms.push((blk, v.clone(), name));
            }
            fx.b.term(Term::Switch {
                value: tag,
                cases,
                default: done,
            });
            for (blk, v, name) in arms {
                fx.b.switch(blk);
                let mut h = word_const(fx, fnv::bytes(name.as_bytes()));
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    let x = fx.ptr_off(a, i64::from(crate::abi::PAYLOAD + *off));
                    let fh = field_hash(fx, x, *ft);
                    h = combine(fx, h, fh);
                }
                fx.set_var(out, h);
                fx.jump(done);
            }
            fx.b.switch(done);
            fx.get_var(out)
        }
        // Nothing else hashes by derivation; the checker refuses it.
        _ => fx.iconst(0),
    }
}

/// A nested record or enum hashes through its own helper, so the recursion is in the
/// generated program.
fn field_hash(fx: &mut Fx<'_, '_>, x: V, t: Ty) -> V {
    match t {
        Ty::Record(_) | Ty::Enum(_) => {
            let h = fx.lw.helper(Helper::Hash(t));
            match fx.call(h, vec![x]) {
                Some(v) => v,
                None => fx.iconst(0),
            }
        }
        _ => hash(fx, x, t),
    }
}

/// A nested record or enum field orders through its own helper, so the recursion is in the
/// generated program.
fn field_order(fx: &mut Fx<'_, '_>, x: V, y: V, t: Ty, less: B, greater: B) {
    match t {
        Ty::Record(_) | Ty::Enum(_) => {
            let h = fx.lw.helper(Helper::Order(t));
            let r = match fx.call(h, vec![x, y]) {
                Some(v) => v,
                None => fx.iconst(0),
            };
            let not_less = fx.b.block();
            let lt = fx.cmp_imm(Cmp::Lt, r, Class::I64, 0);
            fx.branch(lt, less, not_less);
            fx.b.switch(not_less);
            let gt = fx.cmp_imm(Cmp::Gt, r, Class::I64, 0);
            let cont = fx.b.block();
            fx.branch(gt, greater, cont);
            fx.b.switch(cont);
        }
        _ => order(fx, x, y, t, less, greater),
    }
}

/// A nested record or enum field compares through its own helper, so the recursion is in the
/// generated program.
fn field_equal(fx: &mut Fx<'_, '_>, x: V, y: V, t: Ty, differ: B) {
    match t {
        Ty::Record(_) | Ty::Enum(_) => {
            let h = fx.lw.helper(Helper::Equal(t));
            let same = match fx.call(h, vec![x, y]) {
                Some(v) => v,
                None => fx.bit(false),
            };
            next(fx, same, differ);
        }
        _ => equal(fx, x, y, t, differ),
    }
}

/// The `void *(void *)` a thread starts in: unpack the block, call the function with the types
/// that site established, publish a failure into the block, give back the arguments.
pub(crate) fn trampoline(
    lw: &mut Lw<'_>,
    sym: SymId,
    callee: mir::FuncId,
    args: &[(Ty, u32)],
) -> Function {
    let mut fx = Fx::helper(lw);
    let a = fx.param(0);
    let Some(fsym) = fx.lw.funcs.get(&callee).copied() else {
        let at = fx.at;
        fx.lw.defect(at, "a task running an undeclared function");
        let z = fx.null();
        fx.b.term(Term::Return(Some(z)));
        return fx.b.finish(sym, "a task trampoline".to_owned(), None);
    };
    let ret = fx
        .lw
        .native
        .sigs
        .get(callee.index())
        .map_or(Ty::Int, |s| s.ret);
    let mut vals = Vec::new();
    // A task's result is discarded — results travel through channels — so an in-memory one
    // needs somewhere to land.
    if in_memory(ret) {
        let n = fx.size(ret);
        let o = fx.scratch(n);
        vals.push(o);
    }
    for (ty, off) in args {
        let at = fx.ptr_off(a, i64::from(*off));
        if in_memory(*ty) {
            vals.push(at);
        } else {
            let v = fx.load(class(*ty), at, 0);
            vals.push(v);
        }
    }
    fx.call(fsym, vals);
    // A failure ends the task, not the process: the scope's join reports it.
    fx.rt("nz.task_publish", &[a]);
    for (ty, off) in args {
        let at = fx.ptr_off(a, i64::from(*off));
        fx.release_at(at, *ty);
    }
    let z = fx.null();
    fx.b.term(Term::Return(Some(z)));
    let _ = rt::TASK_ARGUMENTS;
    fx.b.finish(sym, "a task trampoline".to_owned(), None)
}

/// An export's C entry (N55, §7.57): the C convention outside, the Nazm function inside, and a
/// failure reported and the process ended rather than a stand-in returned into C.
pub(crate) fn export(
    lw: &mut Lw<'_>,
    sym: SymId,
    callee: mir::FuncId,
    params: &[Ty],
    ret: Ty,
) -> Function {
    let mut fx = Fx::helper(lw);
    let args: Vec<V> = (0..params.len())
        .map(|i| fx.param(u32::try_from(i).unwrap_or(0)))
        .collect();
    let Some(fsym) = fx.lw.funcs.get(&callee).copied() else {
        let at = fx.at;
        fx.lw.defect(at, "an export of an undeclared function");
        let z = fx.iconst_c(class(ret), 0);
        fx.b.term(Term::Return(Some(z)));
        return fx.b.finish(sym, "an export's C entry".to_owned(), None);
    };
    let result = match fx.call(fsym, args) {
        Some(v) => v,
        None => fx.iconst_c(class(ret), 0),
    };
    let failed = fx.rt_v("nz.failed_now", &[], Class::I32);
    let bad = fx.cmp_imm(Cmp::Ne, failed, Class::I32, 0);
    let fail = fx.b.block();
    let done = fx.b.block();
    fx.branch(bad, fail, done);
    fx.b.switch(fail);
    fx.rt("nz.report", &[]);
    fx.b.term(Term::Unreachable);
    fx.b.switch(done);
    fx.b.term(Term::Return(Some(result)));
    fx.b.finish(sym, "an export's C entry".to_owned(), None)
}
