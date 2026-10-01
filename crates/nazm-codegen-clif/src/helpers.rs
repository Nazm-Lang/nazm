//! The helpers a unit's code asked for, and its task trampolines.
//!
//! One function per type per direction — retain, release, equality, and the per-element release
//! the runtime calls when a `Vec` goes — so a record released in forty places is forty calls,
//! and a nested type's helper calls the inner type's: the recursion is in the emitted code, not
//! in this compiler. The same design as the LLVM backend's, over this backend's layout.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Value};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch};
use cranelift_module::Module;
use nazm_lir::{EnumTy, RecordTy, Ty};

use crate::func::{mem, Fx};
use crate::layout::{in_memory, PAYLOAD};
use crate::unit::{Gen, Helper, Trampoline};

fn start<'b>(
    g: &Gen<'_>,
    ctx: &'b mut Context,
    fctx: &'b mut FunctionBuilderContext,
    params: usize,
    ret: Option<cranelift_codegen::ir::Type>,
) -> FunctionBuilder<'b> {
    let mut s = g.module.make_signature();
    for _ in 0..params {
        s.params.push(AbiParam::new(g.ptr));
    }
    if let Some(r) = ret {
        s.returns.push(AbiParam::new(r));
    }
    ctx.func.signature = s;
    FunctionBuilder::new(&mut ctx.func, fctx)
}

pub(crate) fn define(
    g: &mut Gen<'_>,
    ctx: &mut Context,
    fctx: &mut FunctionBuilderContext,
    h: Helper,
) {
    let (params, ret) = match h {
        Helper::Equal(_) => (2, Some(types::I8)),
        _ => (1, None),
    };
    let b = start(g, ctx, fctx, params, ret);
    let mut fx = Fx::helper(g, b);
    let entry = fx.b.create_block();
    fx.b.append_block_params_for_function_params(entry);
    fx.b.switch_to_block(entry);
    let p: Vec<Value> = fx.b.block_params(entry).to_vec();
    let a = p.first().copied().unwrap_or_else(|| fx.iconst(0));
    match h {
        Helper::Retain(t) | Helper::Release(t) => {
            let retain = matches!(h, Helper::Retain(_));
            each_owned(&mut fx, a, t, retain);
            fx.b.ins().return_(&[]);
        }
        Helper::ClosureRelease(f) => {
            let carried: Vec<Ty> =
                fx.g.native
                    .program
                    .function(f)
                    .and_then(|g| g.closure.as_ref())
                    .map(|c| c.iter().map(|t| Ty::of(*t)).collect())
                    .unwrap_or_default();
            let (offsets, _) = fx.closure_layout(&carried);
            for (t, off) in carried.iter().zip(offsets) {
                if fx.g.lay.owns(*t) {
                    let at = fx.b.ins().iadd_imm_s(a, i64::from(off));
                    fx.release_at(at, *t);
                }
            }
            fx.b.ins().return_(&[]);
        }
        Helper::ReleaseElement(t) => {
            if in_memory(t) {
                fx.release_at(a, t);
            } else {
                let v = fx.b.ins().load(types::I64, mem(), a, 0);
                fx.release_value(v, t);
            }
            fx.b.ins().return_(&[]);
        }
        Helper::Equal(t) => {
            let b = p.get(1).copied().unwrap_or(a);
            let differ = fx.b.create_block();
            equal(&mut fx, a, b, t, differ);
            let yes = fx.b.ins().iconst(types::I8, 1);
            fx.b.ins().return_(&[yes]);
            fx.b.switch_to_block(differ);
            let no = fx.b.ins().iconst(types::I8, 0);
            fx.b.ins().return_(&[no]);
        }
    }
    fx.b.seal_all_blocks();
    let cfg = fx.g.module.target_config();
    fx.b.finalize(cfg);
}

/// Retain or release each owning field of the record or enum at `a`.
fn each_owned(fx: &mut Fx<'_, '_, '_>, a: Value, t: Ty, retain: bool) {
    match t {
        Ty::Record(RecordTy(id)) => {
            let Some(s) = fx.g.lay.records.get(&id).cloned() else {
                fx.g.defect(fx.at, "a record with no layout");
                return;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                if !fx.g.lay.owns(*ft) {
                    continue;
                }
                let at = fx.b.ins().iadd_imm_s(a, i64::from(*off));
                one(fx, at, *ft, retain);
            }
        }
        Ty::Enum(EnumTy(id)) => {
            let Some(e) = fx.g.lay.enums.get(&id).cloned() else {
                fx.g.defect(fx.at, "an enum with no layout");
                return;
            };
            // The active variant's fields, and only those: which fields exist is decided at
            // run time, by the discriminant.
            let tag = fx.b.ins().load(types::I64, mem(), a, 0);
            let done = fx.b.create_block();
            let mut sw = Switch::new();
            let mut arms = Vec::new();
            for (tagno, v) in e.variants.iter().enumerate() {
                if !v.fields.iter().any(|f| fx.g.lay.owns(*f)) {
                    continue;
                }
                let blk = fx.b.create_block();
                sw.set_entry(u128::try_from(tagno).unwrap_or(0), blk);
                arms.push((blk, v.clone()));
            }
            sw.emit(&mut fx.b, tag, done);
            for (blk, v) in arms {
                fx.b.switch_to_block(blk);
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    if !fx.g.lay.owns(*ft) {
                        continue;
                    }
                    let at = fx.b.ins().iadd_imm_s(a, i64::from(PAYLOAD + *off));
                    one(fx, at, *ft, retain);
                }
                fx.b.ins().jump(done, &[]);
            }
            fx.b.switch_to_block(done);
        }
        _ => one(fx, a, t, retain),
    }
}

fn one(fx: &mut Fx<'_, '_, '_>, at: Value, t: Ty, retain: bool) {
    if retain {
        fx.retain_at(at, t);
    } else {
        fx.release_at(at, t);
    }
}

/// Continue if the values of type `t` at `a` and `b` are equal; jump to `differ` if not.
fn equal(fx: &mut Fx<'_, '_, '_>, a: Value, b: Value, t: Ty, differ: cranelift_codegen::ir::Block) {
    let next = |fx: &mut Fx<'_, '_, '_>, same: Value| {
        let cont = fx.b.create_block();
        fx.b.ins().brif(same, cont, &[], differ, &[]);
        fx.b.switch_to_block(cont);
    };
    match t {
        Ty::Int | Ty::Bool => {
            let ty = Gen::clif(t);
            let x = fx.b.ins().load(ty, mem(), a, 0);
            let y = fx.b.ins().load(ty, mem(), b, 0);
            let same = fx.b.ins().icmp(IntCC::Equal, x, y);
            next(fx, same);
        }
        Ty::Str => {
            let same = fx.string_equal(a, b);
            next(fx, same);
        }
        Ty::Record(RecordTy(id)) => {
            let Some(s) = fx.g.lay.records.get(&id).cloned() else {
                return;
            };
            for (off, ft) in s.offsets.iter().zip(&s.fields) {
                let x = fx.b.ins().iadd_imm_s(a, i64::from(*off));
                let y = fx.b.ins().iadd_imm_s(b, i64::from(*off));
                field_equal(fx, x, y, *ft, differ);
            }
        }
        Ty::Enum(EnumTy(id)) => {
            let Some(e) = fx.g.lay.enums.get(&id).cloned() else {
                return;
            };
            let ta = fx.b.ins().load(types::I64, mem(), a, 0);
            let tb = fx.b.ins().load(types::I64, mem(), b, 0);
            let same = fx.b.ins().icmp(IntCC::Equal, ta, tb);
            next(fx, same);
            let equal_blk = fx.b.create_block();
            let mut sw = Switch::new();
            let mut arms = Vec::new();
            for (tagno, v) in e.variants.iter().enumerate() {
                if v.fields.is_empty() {
                    continue;
                }
                let blk = fx.b.create_block();
                sw.set_entry(u128::try_from(tagno).unwrap_or(0), blk);
                arms.push((blk, v.clone()));
            }
            sw.emit(&mut fx.b, ta, equal_blk);
            for (blk, v) in arms {
                fx.b.switch_to_block(blk);
                for (off, ft) in v.offsets.iter().zip(&v.fields) {
                    let x = fx.b.ins().iadd_imm_s(a, i64::from(PAYLOAD + *off));
                    let y = fx.b.ins().iadd_imm_s(b, i64::from(PAYLOAD + *off));
                    field_equal(fx, x, y, *ft, differ);
                }
                fx.b.ins().jump(equal_blk, &[]);
            }
            fx.b.switch_to_block(equal_blk);
        }
        // No equality exists for a handle; the checker refuses the comparison.
        _ => {
            fx.b.ins().jump(differ, &[]);
            let dead = fx.b.create_block();
            fx.b.switch_to_block(dead);
        }
    }
}

/// A nested record or enum field compares through its own helper, so the recursion is in the
/// emitted program.
fn field_equal(
    fx: &mut Fx<'_, '_, '_>,
    x: Value,
    y: Value,
    t: Ty,
    differ: cranelift_codegen::ir::Block,
) {
    match t {
        Ty::Record(_) | Ty::Enum(_) => {
            let h = fx.g.helper(Helper::Equal(t));
            let r = fx.func(h);
            let inst = fx.b.ins().call(r, &[x, y]);
            let same =
                fx.b.inst_results(inst)
                    .first()
                    .copied()
                    .unwrap_or_else(|| fx.b.ins().iconst(types::I8, 0));
            let cont = fx.b.create_block();
            fx.b.ins().brif(same, cont, &[], differ, &[]);
            fx.b.switch_to_block(cont);
        }
        _ => equal(fx, x, y, t, differ),
    }
}

/// The `void *(void *)` a thread starts in: unpack the block, call the function with the types
/// that site established, publish a failure into the block, give back the arguments.
pub(crate) fn trampoline(
    g: &mut Gen<'_>,
    ctx: &mut Context,
    fctx: &mut FunctionBuilderContext,
    t: &Trampoline,
) {
    let b = start(g, ctx, fctx, 1, Some(g.ptr));
    let mut fx = Fx::helper(g, b);
    let entry = fx.b.create_block();
    fx.b.append_block_params_for_function_params(entry);
    fx.b.switch_to_block(entry);
    let a =
        fx.b.block_params(entry)
            .first()
            .copied()
            .unwrap_or_else(|| fx.iconst(0));
    let Some(fid) = fx.g.funcs.get(&t.callee).copied() else {
        fx.g.defect(fx.at, "a task running an undeclared function");
        let z = fx.iconst(0);
        fx.b.ins().return_(&[z]);
        fx.b.seal_all_blocks();
        let cfg = fx.g.module.target_config();
        fx.b.finalize(cfg);
        return;
    };
    let ret =
        fx.g.native
            .sigs
            .get(t.callee.index())
            .map_or(Ty::Int, |s| s.ret);
    let mut vals = Vec::new();
    // A task's result is discarded — results travel through channels — so an in-memory one
    // needs somewhere to land.
    if in_memory(ret) {
        let n = fx.size(ret);
        let ss =
            fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                n.max(1),
                3,
            ));
        let o = fx.b.ins().stack_addr(fx.g.ptr, ss, 0);
        vals.push(o);
    }
    for (ty, off) in &t.args {
        let at = fx.b.ins().iadd_imm_s(a, i64::from(*off));
        if in_memory(*ty) {
            vals.push(at);
        } else {
            let v = fx.b.ins().load(Gen::clif(*ty), mem(), at, 0);
            vals.push(v);
        }
    }
    let r = fx.func(fid);
    fx.b.ins().call(r, &vals);
    // A failure ends the task, not the process: the scope's join reports it.
    fx.rt("nz.task_publish", &[a]);
    for (ty, off) in &t.args {
        let at = fx.b.ins().iadd_imm_s(a, i64::from(*off));
        fx.release_at(at, *ty);
    }
    let z = fx.iconst(0);
    fx.b.ins().return_(&[z]);
    fx.b.seal_all_blocks();
    let cfg = fx.g.module.target_config();
    fx.b.finalize(cfg);
}
