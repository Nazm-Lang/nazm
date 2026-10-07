//! LIR as text: what `nazm lir` shows of each unit's instructions, and what a module's digest is
//! of. Deterministic: the same module prints the same bytes.

use std::fmt::Write as _;

use super::ir::{
    BinOp, Class, Cmp, Conv, ConvOp, Inst, Linkage, Module, OvfOp, Sig, Storage, SymId, SymKind,
    Term, UnOp,
};

fn sym(m: &Module, s: SymId) -> String {
    m.symbol(s)
        .map_or_else(|| format!("?{}", s.0), |x| format!("@{}", x.name))
}

fn sig(s: &Sig) -> String {
    let p = s
        .params
        .iter()
        .map(|c| c.name())
        .collect::<Vec<_>>()
        .join(", ");
    let conv = match s.conv {
        Conv::Nazm => "",
        Conv::C => "c ",
    };
    match s.ret {
        Some(r) => format!("{conv}({p}) -> {}", r.name()),
        None => format!("{conv}({p})"),
    }
}

const fn linkage(l: Linkage) -> &'static str {
    match l {
        Linkage::Import => "import",
        Linkage::Local => "local",
        Linkage::Hidden => "hidden",
        Linkage::Export => "export",
    }
}

const fn bin(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::SDiv => "sdiv",
        BinOp::SRem => "srem",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Xor => "xor",
        BinOp::UDiv => "udiv",
        BinOp::URem => "urem",
        BinOp::Shl => "shl",
        BinOp::LShr => "lshr",
        BinOp::AShr => "ashr",
        BinOp::Rotl => "rotl",
        BinOp::Rotr => "rotr",
        BinOp::FAdd => "fadd",
        BinOp::FSub => "fsub",
        BinOp::FMul => "fmul",
        BinOp::FDiv => "fdiv",
    }
}

const fn un(op: UnOp) -> &'static str {
    match op {
        UnOp::FNeg => "fneg",
        UnOp::FSqrt => "fsqrt",
        UnOp::FAbs => "fabs",
        UnOp::FFloor => "ffloor",
        UnOp::FCeil => "fceil",
        UnOp::FTrunc => "ftrunc",
        UnOp::Popcnt => "popcnt",
        UnOp::Clz => "clz",
        UnOp::Ctz => "ctz",
        UnOp::Bswap => "bswap",
    }
}

const fn conv(op: ConvOp) -> &'static str {
    match op {
        ConvOp::SToF => "stof",
        ConvOp::UToF => "utof",
        ConvOp::FToS => "ftos",
        ConvOp::FToU => "ftou",
        ConvOp::FExt => "fext",
        ConvOp::FTrunc => "ftrunc",
        ConvOp::Bits => "bits",
    }
}

const fn ovf(op: OvfOp) -> &'static str {
    match op {
        OvfOp::Add => "add.ovf",
        OvfOp::Sub => "sub.ovf",
        OvfOp::Mul => "mul.ovf",
        OvfOp::UAdd => "uadd.ovf",
        OvfOp::USub => "usub.ovf",
        OvfOp::UMul => "umul.ovf",
    }
}

const fn cmp(op: Cmp) -> &'static str {
    match op {
        Cmp::Eq => "eq",
        Cmp::Ne => "ne",
        Cmp::Lt => "lt",
        Cmp::Le => "le",
        Cmp::Gt => "gt",
        Cmp::Ge => "ge",
        Cmp::ULt => "ult",
        Cmp::ULe => "ule",
        Cmp::UGt => "ugt",
        Cmp::UGe => "uge",
        Cmp::FEq => "feq",
        Cmp::FNe => "fne",
        Cmp::FLt => "flt",
        Cmp::FLe => "fle",
        Cmp::FGt => "fgt",
        Cmp::FGe => "fge",
    }
}

fn args(a: &[super::ir::V]) -> String {
    a.iter()
        .map(|v| format!("v{}", v.0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One instruction.
#[must_use]
pub fn inst(m: &Module, i: &Inst) -> String {
    match i {
        Inst::Param { dst, index } => format!("v{} = param {index}", dst.0),
        Inst::Const { dst, class, value } => format!("v{} = const {} {value}", dst.0, class.name()),
        Inst::Addr { dst, sym: s } => format!("v{} = addr {}", dst.0, sym(m, *s)),
        Inst::SlotAddr { dst, slot } => format!("v{} = slot s{}", dst.0, slot.0),
        Inst::Load {
            dst,
            class,
            addr,
            offset,
        } => format!("v{} = load {} v{}+{offset}", dst.0, class.name(), addr.0),
        Inst::Store {
            addr,
            offset,
            value,
        } => format!("store v{}+{offset}, v{}", addr.0, value.0),
        Inst::Bin { dst, op, a, b } => format!("v{} = {} v{}, v{}", dst.0, bin(*op), a.0, b.0),
        Inst::Ovf {
            dst,
            ovf: o,
            op,
            a,
            b,
        } => format!("v{}, v{} = {} v{}, v{}", dst.0, o.0, ovf(*op), a.0, b.0),
        Inst::Cmp { dst, op, a, b } => format!("v{} = cmp {} v{}, v{}", dst.0, cmp(*op), a.0, b.0),
        Inst::Un { dst, op, a } => format!("v{} = {} v{}", dst.0, un(*op), a.0),
        Inst::Conv { dst, op, to, src } => {
            format!("v{} = {} {} v{}", dst.0, conv(*op), to.name(), src.0)
        }
        Inst::Select { dst, cond, a, b } => {
            format!("v{} = select v{}, v{}, v{}", dst.0, cond.0, a.0, b.0)
        }
        Inst::Ext {
            dst,
            to,
            src,
            signed,
        } => format!(
            "v{} = {} {} v{}",
            dst.0,
            if *signed { "sext" } else { "zext" },
            to.name(),
            src.0
        ),
        Inst::Trunc { dst, to, src } => format!("v{} = trunc {} v{}", dst.0, to.name(), src.0),
        Inst::PtrAdd { dst, base, offset } => {
            format!("v{} = ptradd v{}, v{}", dst.0, base.0, offset.0)
        }
        Inst::PtrToInt { dst, src } => format!("v{} = ptrtoint v{}", dst.0, src.0),
        Inst::IntToPtr { dst, src } => format!("v{} = inttoptr v{}", dst.0, src.0),
        Inst::GetVar { dst, var } => format!("v{} = get x{}", dst.0, var.0),
        Inst::SetVar { var, src } => format!("set x{}, v{}", var.0, src.0),
        Inst::Call {
            dst,
            callee,
            args: a,
        } => match dst {
            Some(d) => format!("v{} = call {}({})", d.0, sym(m, *callee), args(a)),
            None => format!("call {}({})", sym(m, *callee), args(a)),
        },
        Inst::CallIndirect {
            dst,
            addr,
            sig: s,
            args: a,
        } => match dst {
            Some(d) => format!(
                "v{} = call_indirect {} v{}({})",
                d.0,
                sig(s),
                addr.0,
                args(a)
            ),
            None => format!("call_indirect {} v{}({})", sig(s), addr.0, args(a)),
        },
        Inst::Copy { dst, src, size } => format!("copy v{}, v{}, {size}", dst.0, src.0),
        Inst::Zero { dst, size } => format!("zero v{}, {size}", dst.0),
        Inst::Loc(p) => format!("loc {} {} {}", p.file, p.line, p.column),
    }
}

/// One terminator.
#[must_use]
pub fn term(t: &Term) -> String {
    match t {
        Term::Jump(b) => format!("jump b{}", b.0),
        Term::Branch {
            cond,
            then,
            otherwise,
        } => format!("branch v{}, b{}, b{}", cond.0, then.0, otherwise.0),
        Term::Switch {
            value,
            cases,
            default,
        } => {
            let c = cases
                .iter()
                .map(|(k, b)| format!("{k}: b{}", b.0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("switch v{} [{c}] else b{}", value.0, default.0)
        }
        Term::Return(Some(v)) => format!("return v{}", v.0),
        Term::Return(None) => "return".to_owned(),
        Term::Unreachable => "unreachable".to_owned(),
    }
}

/// A whole module.
#[must_use]
pub fn module(m: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "; unit {}", m.source);
    for s in &m.symbols {
        match &s.kind {
            SymKind::Func { sig: g } => {
                let _ = writeln!(out, "{} func @{} {}", linkage(s.linkage), s.name, sig(g));
            }
            SymKind::Data { class } => {
                let c = class.map_or("bytes", Class::name);
                let _ = writeln!(out, "{} data @{} {c}", linkage(s.linkage), s.name);
            }
        }
    }
    for d in &m.data {
        let hex: String = d.bytes.iter().map(|b| format!("{b:02x}")).collect();
        let relocs = d
            .relocs
            .iter()
            .map(|(o, s)| format!(" {o}:{}", sym(m, *s)))
            .collect::<String>();
        let _ = writeln!(
            out,
            "data {} align {} [{hex}]{relocs}",
            sym(m, d.sym),
            d.align
        );
    }
    for f in &m.functions {
        let _ = writeln!(out, "\nfunction {} ; {}", sym(m, f.sym), f.name);
        if let Some((file, line)) = f.at {
            let _ = writeln!(out, "  at {file} {line}");
        }
        for (i, c) in f.vars.iter().enumerate() {
            let _ = writeln!(out, "  var x{i} {}", c.name());
        }
        for (i, s) in f.slots.iter().enumerate() {
            let _ = writeln!(out, "  slot s{i} {} align {}", s.size, s.align);
        }
        for v in &f.debug_vars {
            let storage = match v.storage {
                Storage::Var(x) => format!("x{}", x.0),
                Storage::Slot(s) => format!("s{}", s.0),
                Storage::Indirect(x) => format!("x{}*", x.0),
            };
            let _ = writeln!(
                out,
                "  debug {storage} {} {} {} {:?} {}",
                v.arg, v.line, v.kind, v.scope, v.name
            );
        }
        for (i, b) in f.blocks.iter().enumerate() {
            let _ = writeln!(out, "b{i}:");
            for x in &b.insts {
                let _ = writeln!(out, "  {}", inst(m, x));
            }
            let _ = writeln!(out, "  {}", term(&b.term));
        }
    }
    out
}
