//! The functions a unit's code asks for: helpers, task trampolines, and export C entries.
//!
//! One helper per type per direction — retain, release, equality, and the per-element release
//! the runtime calls when a `Vec` or a generic channel goes — so a record released in forty
//! places is forty calls, and a nested type's helper calls the inner type's: the recursion is in
//! the generated code, not in this compiler.

use nazm_mir as mir;

use super::func::Fx;
use super::ir::{Class, Cmp, Function, SymId, Term, B, V};
use super::lower::{class, in_memory, rt, Helper, Lw};
use crate::ir::{EnumTy, RecordTy, Ty};

pub(crate) fn define(lw: &mut Lw<'_>, sym: SymId, h: Helper) -> Function {
    let mut fx = Fx::helper(lw);
    let a = fx.param(0);
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
        // No equality exists for a handle; the checker refuses the comparison.
        _ => {
            fx.jump(differ);
            let dead = fx.b.block();
            fx.b.switch(dead);
        }
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
