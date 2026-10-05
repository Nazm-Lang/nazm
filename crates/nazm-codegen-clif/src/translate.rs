//! One LIR module, as one object (N105, `docs/architecture.md` §7.106).
//!
//! Each LIR instruction is the Cranelift instruction it names; each block a Cranelift block; each
//! variable a Cranelift variable and each slot a stack slot. Nothing here decides anything a
//! program can observe: which runtime entry is called, every guard, every offset and every
//! failure text are already in the module.

use std::collections::BTreeMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types, AbiParam, Block, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlotData,
    StackSlotKind, TrapCode, Type, Value,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use nazm_diag::{codes, Diagnostic};
use nazm_lir::op::ir::{self as lir, BinOp, Class, Cmp, Conv, Inst, OvfOp, Sig, SymKind, Term};
use nazm_lir::SourceNames;
use nazm_span::Span;

/// A symbol as declared in the object.
#[derive(Clone, Copy)]
enum Declared {
    Func(FuncId),
    Data(DataId),
}

fn defect(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        codes::COMPILER_DEFECT,
        Span::synthetic(),
        format!(
            "the Cranelift backend could not translate LIR: {}",
            what.into()
        ),
    )
    .help("this is a bug in the compiler, not in the program")
}

fn mem() -> MemFlagsData {
    MemFlagsData::trusted()
}

struct Tr {
    module: ObjectModule,
    ptr: Type,
    declared: Vec<Declared>,
    errors: Vec<Diagnostic>,
    debug: Option<crate::dwarf::Debug>,
}

impl Tr {
    fn ty(&self, c: Class) -> Type {
        match c {
            Class::Bit | Class::I8 => types::I8,
            Class::I32 => types::I32,
            Class::I64 => types::I64,
            Class::Ptr => self.ptr,
        }
    }

    fn signature(&self, s: &Sig) -> Signature {
        let mut out = self.module.make_signature();
        let param = |c: Class| abi_param(self.ty(c), c, s.conv);
        out.params.extend(s.params.iter().map(|c| param(*c)));
        if let Some(r) = s.ret {
            out.returns.push(param(r));
        }
        out
    }
}

/// One parameter or result as Cranelift passes it. C's `bool` is a byte the caller zero-extends
/// (N42); the internal convention promises nothing about the bits above it. On arm64 a missing
/// extension is invisible to any program that only passes constants and comparisons, which is
/// why `tests::a_c_bool_is_zero_extended_and_an_internal_one_is_not` holds it directly.
fn abi_param(t: Type, c: Class, conv: Conv) -> AbiParam {
    let p = AbiParam::new(t);
    if conv == Conv::C && c == Class::Bit {
        p.uext()
    } else {
        p
    }
}

const fn linkage(l: lir::Linkage) -> Linkage {
    match l {
        lir::Linkage::Import => Linkage::Import,
        lir::Linkage::Local => Linkage::Local,
        lir::Linkage::Hidden => Linkage::Hidden,
        lir::Linkage::Export => Linkage::Export,
    }
}

/// One module's object.
pub(crate) fn object(
    isa: OwnedTargetIsa,
    m: &lir::Module,
    names: &SourceNames<'_>,
    name: &str,
    debug: bool,
    verify: bool,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let builder = ObjectBuilder::new(isa, name, cranelift_module::default_libcall_names())
        .map_err(|e| vec![defect(e.to_string())])?;
    let module = ObjectModule::new(builder);
    let ptr = module.target_config().pointer_type();
    let mut t = Tr {
        module,
        ptr,
        declared: Vec::with_capacity(m.symbols.len()),
        errors: Vec::new(),
        debug: debug.then(crate::dwarf::Debug::default),
    };
    for s in &m.symbols {
        let d = match &s.kind {
            SymKind::Func { sig } => {
                let sg = t.signature(sig);
                match t.module.declare_function(&s.name, linkage(s.linkage), &sg) {
                    Ok(f) => Declared::Func(f),
                    Err(e) => {
                        t.errors.push(defect(format!("`{}`: {e}", s.name)));
                        Declared::Func(FuncId::from_u32(0))
                    }
                }
            }
            SymKind::Data { .. } => {
                let writable = s.linkage == lir::Linkage::Import;
                match t
                    .module
                    .declare_data(&s.name, linkage(s.linkage), writable, false)
                {
                    Ok(d) => Declared::Data(d),
                    Err(e) => {
                        t.errors.push(defect(format!("`{}`: {e}", s.name)));
                        Declared::Data(DataId::from_u32(0))
                    }
                }
            }
        };
        t.declared.push(d);
    }
    for d in &m.data {
        let Some(Declared::Data(id)) = t.declared.get(d.sym.index()).copied() else {
            t.errors
                .push(defect("data under a symbol that is not data"));
            continue;
        };
        let mut desc = DataDescription::new();
        desc.define(d.bytes.clone().into_boxed_slice());
        desc.set_align(u64::from(d.align.max(1)));
        for (off, s) in &d.relocs {
            if let Some(Declared::Data(target)) = t.declared.get(s.index()).copied() {
                let gv = t.module.declare_data_in_data(target, &mut desc);
                desc.write_data_addr(*off, gv, 0);
            } else {
                t.errors
                    .push(defect("a data relocation to a symbol that is not data"));
            }
        }
        if let Err(e) = t.module.define_data(id, &desc) {
            t.errors.push(defect(e.to_string()));
        }
    }
    let mut fctx = FunctionBuilderContext::new();
    for f in &m.functions {
        let Some(Declared::Func(id)) = t.declared.get(f.sym.index()).copied() else {
            t.errors
                .push(defect("a function under a symbol that is not a function"));
            continue;
        };
        let Some(sig) = m.sig(f.sym).cloned() else {
            continue;
        };
        let mut ctx = t.module.make_context();
        ctx.func.signature = t.signature(&sig);
        function(&mut t, m, &mut ctx, &mut fctx, f);
        if verify {
            if let Err(e) = cranelift_codegen::verify_function(&ctx.func, t.module.isa()) {
                t.errors.push(defect(format!(
                    "Cranelift's verifier rejected `{}`: {e}",
                    f.name
                )));
                continue;
            }
        }
        if let Err(e) = t.module.define_function(id, &mut ctx) {
            t.errors.push(defect(format!("`{}`: {e:?}", f.name)));
            continue;
        }
        describe(&mut t, id, &ctx, f);
    }
    if !t.errors.is_empty() {
        return Err(t.errors);
    }
    let mut product = t.module.finish();
    if let Some(debug) = &t.debug {
        if let Err(e) = crate::dwarf::write(&mut product, debug, &|i| names.debug_file(i)) {
            return Err(vec![defect(format!(
                "could not write debug information: {e}"
            ))]);
        }
    }
    let bytes = product
        .emit()
        .map_err(|e| vec![defect(format!("could not write an object: {e}"))])?;
    if debug {
        crate::dwarf::settle_macho(bytes).map_err(|e| vec![defect(e)])
    } else {
        Ok(bytes)
    }
}

/// A function's debug facts, in a `--debug` build: its declaration, its code's length, and the
/// code range each location covers (N91).
fn describe(t: &mut Tr, id: FuncId, ctx: &Context, f: &lir::Function) {
    let (Some(debug), Some(code), Some((file, line))) =
        (t.debug.as_mut(), ctx.compiled_code(), f.at)
    else {
        return;
    };
    let ranges = code
        .buffer
        .get_srclocs_sorted()
        .iter()
        .filter(|r| !r.loc.is_default())
        .map(|r| (r.start, r.end, r.loc.bits()))
        .collect();
    debug.functions.push(crate::dwarf::Function {
        func: id,
        name: f.name.clone(),
        file,
        line,
        size: code.code_info().total_size,
        ranges,
    });
}

struct Fn<'t, 'b> {
    t: &'t mut Tr,
    b: FunctionBuilder<'b>,
    refs: BTreeMap<FuncId, FuncRef>,
    vals: Vec<Option<Value>>,
    classes: Vec<Class>,
}

impl Fn<'_, '_> {
    fn v(&mut self, v: lir::V) -> Value {
        match self.vals.get(v.index()).copied().flatten() {
            Some(x) => x,
            None => {
                self.t
                    .errors
                    .push(defect(format!("v{} is used before it is defined", v.0)));
                self.b.ins().iconst(types::I64, 0)
            }
        }
    }

    fn def(&mut self, v: lir::V, x: Value, c: Class) {
        if let Some(slot) = self.vals.get_mut(v.index()) {
            *slot = Some(x);
        }
        if let Some(slot) = self.classes.get_mut(v.index()) {
            *slot = c;
        }
    }

    fn class(&self, v: lir::V) -> Class {
        self.classes.get(v.index()).copied().unwrap_or(Class::I64)
    }

    fn func(&mut self, id: FuncId) -> FuncRef {
        if let Some(r) = self.refs.get(&id) {
            return *r;
        }
        let r = self.t.module.declare_func_in_func(id, self.b.func);
        self.refs.insert(id, r);
        r
    }
}

fn function(
    t: &mut Tr,
    m: &lir::Module,
    ctx: &mut Context,
    fctx: &mut FunctionBuilderContext,
    f: &lir::Function,
) {
    let b = FunctionBuilder::new(&mut ctx.func, fctx);
    let mut fx = Fn {
        t,
        b,
        refs: BTreeMap::new(),
        vals: vec![None; f.values as usize],
        classes: vec![Class::I64; f.values as usize],
    };
    let vars: Vec<Variable> = f
        .vars
        .iter()
        .map(|c| {
            let ty = fx.t.ty(*c);
            fx.b.declare_var(ty)
        })
        .collect();
    let blocks: Vec<Block> = f.blocks.iter().map(|_| fx.b.create_block()).collect();
    let Some(entry) = blocks.first().copied() else {
        fx.t.errors.push(defect("a function with no blocks"));
        return;
    };
    fx.b.append_block_params_for_function_params(entry);
    let params: Vec<Value> = fx.b.block_params(entry).to_vec();
    let slots: Vec<_> = f
        .slots
        .iter()
        .map(|s| {
            let shift = u8::try_from(s.align.max(1).trailing_zeros()).unwrap_or(3);
            fx.b.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                s.size,
                shift,
            ))
        })
        .collect();
    for (bi, blk) in f.blocks.iter().enumerate() {
        let Some(cb) = blocks.get(bi).copied() else {
            continue;
        };
        fx.b.switch_to_block(cb);
        for i in &blk.insts {
            inst(&mut fx, m, f, i, &params, &vars, &slots);
        }
        let block = |b: lir::B| blocks.get(b.index()).copied();
        match &blk.term {
            Term::Jump(to) => match block(*to) {
                Some(to) => {
                    fx.b.ins().jump(to, &[]);
                }
                None => fx.t.errors.push(defect("a jump to no block")),
            },
            Term::Branch {
                cond,
                then,
                otherwise,
            } => {
                let c = fx.v(*cond);
                match (block(*then), block(*otherwise)) {
                    (Some(a), Some(z)) => {
                        fx.b.ins().brif(c, a, &[], z, &[]);
                    }
                    _ => fx.t.errors.push(defect("a branch to no block")),
                }
            }
            Term::Switch {
                value,
                cases,
                default,
            } => {
                let x = fx.v(*value);
                let mut sw = Switch::new();
                for (k, to) in cases {
                    if let (Ok(k), Some(to)) = (u128::try_from(*k), block(*to)) {
                        sw.set_entry(k, to);
                    } else {
                        fx.t.errors
                            .push(defect("a switch case that is negative or to no block"));
                    }
                }
                match block(*default) {
                    Some(d) => sw.emit(&mut fx.b, x, d),
                    None => fx.t.errors.push(defect("a switch default to no block")),
                }
            }
            Term::Return(v) => {
                let r: Vec<Value> = v.iter().map(|v| fx.v(*v)).collect();
                fx.b.ins().return_(&r);
            }
            Term::Unreachable => {
                fx.b.ins().trap(TrapCode::unwrap_user(1));
            }
        }
    }
    fx.b.seal_all_blocks();
    let cfg = fx.t.module.target_config();
    fx.b.finalize(cfg);
}

#[allow(clippy::too_many_lines)]
fn inst(
    fx: &mut Fn<'_, '_>,
    m: &lir::Module,
    f: &lir::Function,
    i: &Inst,
    params: &[Value],
    vars: &[Variable],
    slots: &[cranelift_codegen::ir::StackSlot],
) {
    match i {
        Inst::Param { dst, index } => {
            let x = params
                .get(*index as usize)
                .copied()
                .unwrap_or_else(|| fx.b.ins().iconst(types::I64, 0));
            let class = m
                .sig(f.sym)
                .and_then(|s| s.params.get(*index as usize).copied())
                .unwrap_or(Class::I64);
            fx.def(*dst, x, class);
        }
        Inst::Const { dst, class, value } => {
            let ty = fx.t.ty(*class);
            let x = fx.b.ins().iconst(ty, *value);
            fx.def(*dst, x, *class);
        }
        Inst::Addr { dst, sym } => {
            let ptr = fx.t.ptr;
            let x = match fx.t.declared.get(sym.index()).copied() {
                Some(Declared::Func(id)) => {
                    let r = fx.func(id);
                    fx.b.ins().func_addr(ptr, r)
                }
                Some(Declared::Data(id)) => {
                    let gv = fx.t.module.declare_data_in_func(id, fx.b.func);
                    fx.b.ins().symbol_value(ptr, gv)
                }
                None => {
                    fx.t.errors.push(defect("the address of no symbol"));
                    fx.b.ins().iconst(ptr, 0)
                }
            };
            fx.def(*dst, x, Class::Ptr);
        }
        Inst::SlotAddr { dst, slot } => {
            let ptr = fx.t.ptr;
            let x = match slots.get(slot.0 as usize) {
                Some(s) => fx.b.ins().stack_addr(ptr, *s, 0),
                None => fx.b.ins().iconst(ptr, 0),
            };
            fx.def(*dst, x, Class::Ptr);
        }
        Inst::Load {
            dst,
            class,
            addr,
            offset,
        } => {
            let a = fx.v(*addr);
            let ty = fx.t.ty(*class);
            let x = fx.b.ins().load(ty, mem(), a, *offset);
            fx.def(*dst, x, *class);
        }
        Inst::Store {
            addr,
            offset,
            value,
        } => {
            let a = fx.v(*addr);
            let x = fx.v(*value);
            fx.b.ins().store(mem(), x, a, *offset);
        }
        Inst::Bin { dst, op, a, b } => {
            let (x, y) = (fx.v(*a), fx.v(*b));
            let c = fx.class(*a);
            let r = match op {
                BinOp::Add => fx.b.ins().iadd(x, y),
                BinOp::Sub => fx.b.ins().isub(x, y),
                BinOp::Mul => fx.b.ins().imul(x, y),
                BinOp::SDiv => fx.b.ins().sdiv(x, y),
                BinOp::SRem => fx.b.ins().srem(x, y),
                BinOp::And => fx.b.ins().band(x, y),
                BinOp::Or => fx.b.ins().bor(x, y),
                BinOp::Xor => fx.b.ins().bxor(x, y),
            };
            fx.def(*dst, r, c);
        }
        Inst::Ovf { dst, ovf, op, a, b } => {
            let (x, y) = (fx.v(*a), fx.v(*b));
            let (r, o) = match op {
                OvfOp::Add => fx.b.ins().sadd_overflow(x, y),
                OvfOp::Sub => fx.b.ins().ssub_overflow(x, y),
                OvfOp::Mul => fx.b.ins().smul_overflow(x, y),
            };
            fx.def(*dst, r, Class::I64);
            fx.def(*ovf, o, Class::Bit);
        }
        Inst::Cmp { dst, op, a, b } => {
            let (x, y) = (fx.v(*a), fx.v(*b));
            let cc = match op {
                Cmp::Eq => IntCC::Equal,
                Cmp::Ne => IntCC::NotEqual,
                Cmp::Lt => IntCC::SignedLessThan,
                Cmp::Le => IntCC::SignedLessThanOrEqual,
                Cmp::Gt => IntCC::SignedGreaterThan,
                Cmp::Ge => IntCC::SignedGreaterThanOrEqual,
            };
            let r = fx.b.ins().icmp(cc, x, y);
            fx.def(*dst, r, Class::Bit);
        }
        Inst::Select { dst, cond, a, b } => {
            let (c, x, y) = (fx.v(*cond), fx.v(*a), fx.v(*b));
            let cl = fx.class(*a);
            let r = fx.b.ins().select(c, x, y);
            fx.def(*dst, r, cl);
        }
        Inst::Ext {
            dst,
            to,
            src,
            signed,
        } => {
            let x = fx.v(*src);
            let ty = fx.t.ty(*to);
            let r = if fx.b.func.dfg.value_type(x) == ty {
                x
            } else if *signed {
                fx.b.ins().sextend(ty, x)
            } else {
                fx.b.ins().uextend(ty, x)
            };
            fx.def(*dst, r, *to);
        }
        Inst::Trunc { dst, to, src } => {
            let x = fx.v(*src);
            let ty = fx.t.ty(*to);
            let r = if fx.b.func.dfg.value_type(x) == ty {
                x
            } else {
                fx.b.ins().ireduce(ty, x)
            };
            fx.def(*dst, r, *to);
        }
        Inst::PtrAdd { dst, base, offset } => {
            let (x, mut o) = (fx.v(*base), fx.v(*offset));
            if fx.t.ptr != types::I64 {
                o = fx.b.ins().ireduce(fx.t.ptr, o);
            }
            let r = fx.b.ins().iadd(x, o);
            fx.def(*dst, r, Class::Ptr);
        }
        Inst::PtrToInt { dst, src } => {
            let x = fx.v(*src);
            let r = if fx.t.ptr == types::I64 {
                x
            } else {
                fx.b.ins().uextend(types::I64, x)
            };
            fx.def(*dst, r, Class::I64);
        }
        Inst::IntToPtr { dst, src } => {
            let x = fx.v(*src);
            let r = if fx.t.ptr == types::I64 {
                x
            } else {
                fx.b.ins().ireduce(fx.t.ptr, x)
            };
            fx.def(*dst, r, Class::Ptr);
        }
        Inst::GetVar { dst, var } => {
            let Some(v) = vars.get(var.0 as usize).copied() else {
                fx.t.errors.push(defect("no such variable"));
                return;
            };
            let x = fx.b.use_var(v);
            let c = f.vars.get(var.0 as usize).copied().unwrap_or(Class::I64);
            fx.def(*dst, x, c);
        }
        Inst::SetVar { var, src } => {
            let x = fx.v(*src);
            if let Some(v) = vars.get(var.0 as usize).copied() {
                fx.b.def_var(v, x);
            }
        }
        Inst::Call { dst, callee, args } => {
            let Some(Declared::Func(id)) = fx.t.declared.get(callee.index()).copied() else {
                fx.t.errors
                    .push(defect("a call to a symbol that is not a function"));
                return;
            };
            let a: Vec<Value> = args.iter().map(|v| fx.v(*v)).collect();
            let r = fx.func(id);
            let call = fx.b.ins().call(r, &a);
            let result = fx.b.inst_results(call).first().copied();
            if let (Some(d), Some(x)) = (dst, result) {
                let class = m.sig(*callee).and_then(|s| s.ret).unwrap_or(Class::I64);
                // A bit returned by code another compiler built (the runtime's `i1`) promises
                // only its low bit.
                let x = if class == Class::Bit {
                    fx.b.ins().band_imm_s(x, 1)
                } else {
                    x
                };
                fx.def(*d, x, class);
            }
        }
        Inst::CallIndirect {
            dst,
            addr,
            sig,
            args,
        } => {
            let code = fx.v(*addr);
            let a: Vec<Value> = args.iter().map(|v| fx.v(*v)).collect();
            let s = fx.t.signature(sig);
            let sref = fx.b.import_signature(s);
            let call = fx.b.ins().call_indirect(sref, code, &a);
            if let (Some(d), Some(x)) = (dst, fx.b.inst_results(call).first().copied()) {
                fx.def(*d, x, sig.ret.unwrap_or(Class::I64));
            }
        }
        Inst::Copy { dst, src, size } => {
            let (d, s) = (fx.v(*dst), fx.v(*src));
            let cfg = fx.t.module.target_config();
            fx.b.emit_small_memory_copy(cfg, d, s, u64::from(*size), 1, 1, true, mem());
        }
        Inst::Zero { dst, size } => {
            let d = fx.v(*dst);
            let cfg = fx.t.module.target_config();
            fx.b.emit_small_memset(cfg, d, 0, u64::from(*size), 1, mem());
        }
        Inst::Loc(p) => {
            if let Some(debug) = fx.t.debug.as_mut() {
                let i = debug.location((p.file, p.line, p.column));
                fx.b.set_srcloc(cranelift_codegen::ir::SourceLoc::new(i));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{abi_param, types, Class, Conv};
    use cranelift_codegen::ir::ArgumentExtension;

    #[test]
    fn a_c_bool_is_zero_extended_and_an_internal_one_is_not() {
        assert_eq!(
            abi_param(types::I8, Class::Bit, Conv::C).extension,
            ArgumentExtension::Uext
        );
        assert_eq!(
            abi_param(types::I8, Class::Bit, Conv::Nazm).extension,
            ArgumentExtension::None
        );
        assert_eq!(
            abi_param(types::I8, Class::I8, Conv::C).extension,
            ArgumentExtension::None
        );
    }
}
