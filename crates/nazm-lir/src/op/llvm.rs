//! LIR as LLVM IR text (N105, `docs/architecture.md` §7.106): the LLVM backend's whole job.
//!
//! Each LIR instruction is the LLVM instruction it names; each variable and each slot an
//! `alloca` in the entry block, which LLVM's own promotion turns into registers. Nothing here
//! decides anything a program can observe.

use std::fmt::Write as _;

use super::ir::{
    BinOp, Class, Cmp, Conv, Function, Inst, Linkage, Module, OvfOp, Sig, Storage, SymId, SymKind,
    Term, V,
};

const fn ty(c: Class) -> &'static str {
    match c {
        Class::Bit => "i1",
        Class::I8 => "i8",
        Class::I32 => "i32",
        Class::I64 => "i64",
        Class::Ptr => "ptr",
    }
}

/// A parameter's or result's type in a signature: C's `bool` is zero-extended (N42).
fn sig_ty(c: Class, conv: Conv, result: bool) -> String {
    match (c, conv, result) {
        (Class::Bit, Conv::C, true) => "zeroext i1".to_owned(),
        (Class::Bit, Conv::C, false) => "i1 zeroext".to_owned(),
        _ => ty(c).to_owned(),
    }
}

fn ret_ty(s: &Sig) -> String {
    s.ret
        .map_or_else(|| "void".to_owned(), |c| sig_ty(c, s.conv, true))
}

/// LLVM's constant-string escaping.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes {
        if *b == b'"' || *b == b'\\' || !(0x20..0x7f).contains(b) {
            let _ = write!(out, "\\{b:02X}");
        } else {
            out.push(*b as char);
        }
    }
    out
}

/// A symbol's name as LLVM spells it: bare where every byte is one LLVM accepts bare, quoted
/// otherwise.
fn name(m: &Module, s: SymId) -> String {
    let n = m.symbol(s).map_or("nz.missing", |x| x.name.as_str());
    if n.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'$' | b'-'))
    {
        format!("@{n}")
    } else {
        format!("@\"{n}\"")
    }
}

/// The text of one module.
#[must_use]
pub fn module(m: &Module) -> String {
    let mut out = String::new();
    crate::runtime::preamble(&mut out, &m.source);
    let _ = writeln!(
        out,
        "declare {{i64, i1}} @llvm.sadd.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        out,
        "declare {{i64, i1}} @llvm.ssub.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        out,
        "declare {{i64, i1}} @llvm.smul.with.overflow.i64(i64, i64)"
    );
    let _ = writeln!(
        out,
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)"
    );
    let _ = writeln!(out, "declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)");
    if m.symbols.iter().any(|s| s.name == "nz.failed_now") {
        let _ = writeln!(out, "@nz.failed = external thread_local global i1");
    }
    for (i, s) in m.symbols.iter().enumerate() {
        if s.linkage != Linkage::Import {
            continue;
        }
        let id = SymId(u32::try_from(i).unwrap_or(u32::MAX));
        match &s.kind {
            SymKind::Func { sig } => {
                let params = sig
                    .params
                    .iter()
                    .map(|c| sig_ty(*c, sig.conv, false))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "declare {} {}({params})", ret_ty(sig), name(m, id));
            }
            SymKind::Data { class } => {
                let _ = writeln!(
                    out,
                    "{} = external global {}",
                    name(m, id),
                    ty(class.unwrap_or(Class::I8))
                );
            }
        }
    }
    out.push('\n');
    for d in &m.data {
        let mut parts: Vec<String> = Vec::new();
        let mut at = 0usize;
        let mut relocs = d.relocs.clone();
        relocs.sort_by_key(|r| r.0);
        for (off, s) in &relocs {
            let off = *off as usize;
            if off > at {
                let b = d.bytes.get(at..off).unwrap_or(&[]);
                parts.push(format!("[{} x i8] c\"{}\"", b.len(), escape(b)));
            }
            parts.push(format!("ptr {}", name(m, *s)));
            at = off + m.pointer as usize;
        }
        if at < d.bytes.len() {
            let b = d.bytes.get(at..).unwrap_or(&[]);
            parts.push(format!("[{} x i8] c\"{}\"", b.len(), escape(b)));
        }
        let body = if relocs.is_empty() {
            parts.join("")
        } else {
            let types = parts
                .iter()
                .map(|p| {
                    p.split_once(" c\"")
                        .map_or_else(|| "ptr".to_owned(), |(t, _)| t.to_owned())
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("<{{ {types} }}> <{{ {} }}>", parts.join(", "))
        };
        let _ = writeln!(
            out,
            "{} = private unnamed_addr constant {body}, align {}",
            name(m, d.sym),
            d.align.max(1)
        );
    }
    out.push('\n');
    for f in &m.functions {
        function(&mut out, m, f);
    }
    out
}

struct Pr<'m> {
    m: &'m Module,
    vals: Vec<Option<String>>,
    classes: Vec<Class>,
    temp: u32,
}

impl Pr<'_> {
    fn v(&self, v: V) -> String {
        self.vals
            .get(v.index())
            .cloned()
            .flatten()
            .unwrap_or_else(|| "undef".to_owned())
    }

    fn c(&self, v: V) -> Class {
        self.classes.get(v.index()).copied().unwrap_or(Class::I64)
    }

    /// `ty value`, as an operand.
    fn tv(&self, v: V) -> String {
        format!("{} {}", ty(self.c(v)), self.v(v))
    }

    fn named(&mut self, v: V, c: Class) -> String {
        let n = format!("%v{}", v.0);
        if let Some(slot) = self.vals.get_mut(v.index()) {
            *slot = Some(n.clone());
        }
        if let Some(slot) = self.classes.get_mut(v.index()) {
            *slot = c;
        }
        n
    }

    fn alias(&mut self, v: V, text: String, c: Class) {
        if let Some(slot) = self.vals.get_mut(v.index()) {
            *slot = Some(text);
        }
        if let Some(slot) = self.classes.get_mut(v.index()) {
            *slot = c;
        }
    }

    fn temp(&mut self) -> String {
        self.temp += 1;
        format!("%t{}", self.temp)
    }

    /// An address `offset` bytes past `addr`.
    fn at(&mut self, out: &mut String, addr: V, offset: i32) -> String {
        if offset == 0 {
            return self.v(addr);
        }
        let t = self.temp();
        let _ = writeln!(
            out,
            "  {t} = getelementptr i8, ptr {}, i64 {offset}",
            self.v(addr)
        );
        t
    }
}

/// A block's label: the entry is `entry`, as LLVM text conventionally has it.
fn label(b: usize) -> String {
    if b == 0 {
        "entry".to_owned()
    } else {
        format!("b{b}")
    }
}

fn linkage(l: Linkage) -> &'static str {
    match l {
        Linkage::Local => "internal ",
        Linkage::Hidden => "hidden ",
        Linkage::Export | Linkage::Import => "",
    }
}

#[allow(clippy::too_many_lines)]
fn function(out: &mut String, m: &Module, f: &Function) {
    let Some(sym) = m.symbol(f.sym) else { return };
    let Some(sig) = m.sig(f.sym) else { return };
    let params = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} %p{i}", sig_ty(*c, sig.conv, false)))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        out,
        "define {}{} {}({params}) {{   ; {}",
        linkage(sym.linkage),
        ret_ty(sig),
        name(m, f.sym),
        f.name
    );
    let mut p = Pr {
        m,
        vals: vec![None; f.values as usize],
        classes: vec![Class::I64; f.values as usize],
        temp: 0,
    };
    for (bi, b) in f.blocks.iter().enumerate() {
        let _ = writeln!(out, "{}:", label(bi));
        if bi == 0 {
            if let Some((file, line)) = f.at {
                let _ = writeln!(out, "  ; nz.fn {file} {line}");
            }
            for (i, c) in f.vars.iter().enumerate() {
                let note = f.var_names.get(i).filter(|n| !n.is_empty());
                let _ = match note {
                    Some(n) => writeln!(out, "  %x{i} = alloca {}            ; {n}", ty(*c)),
                    None => writeln!(out, "  %x{i} = alloca {}", ty(*c)),
                };
            }
            for (i, s) in f.slots.iter().enumerate() {
                let note = f.slot_names.get(i).filter(|n| !n.is_empty());
                let _ = match note {
                    Some(n) => writeln!(
                        out,
                        "  %s{i} = alloca [{} x i8], align {}            ; {n}",
                        s.size,
                        s.align.max(1)
                    ),
                    None => writeln!(
                        out,
                        "  %s{i} = alloca [{} x i8], align {}",
                        s.size,
                        s.align.max(1)
                    ),
                };
            }
            for v in &f.debug_vars {
                let storage = match v.storage {
                    Storage::Var(x) => format!("x{}", x.0),
                    Storage::Slot(s) => format!("s{}", s.0),
                    Storage::Indirect(x) => format!("x{}*", x.0),
                };
                let _ = writeln!(
                    out,
                    "  ; nz.var {storage} {} {} {} {} {} {} {} {}",
                    v.arg, v.line, v.kind, v.scope.0, v.scope.1, v.scope.2, v.scope.3, v.name
                );
            }
        }
        for i in &b.insts {
            inst(out, &mut p, sig, &f.vars, i);
        }
        match &b.term {
            Term::Jump(t) => {
                let _ = writeln!(out, "  br label %{}", label(t.index()));
            }
            Term::Branch {
                cond,
                then,
                otherwise,
            } => {
                let _ = writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}",
                    p.v(*cond),
                    label(then.index()),
                    label(otherwise.index())
                );
            }
            Term::Switch {
                value,
                cases,
                default,
            } => {
                let _ = writeln!(
                    out,
                    "  switch {}, label %{} [",
                    p.tv(*value),
                    label(default.index())
                );
                let vt = ty(p.c(*value));
                for (k, t) in cases {
                    let _ = writeln!(out, "    {vt} {k}, label %{}", label(t.index()));
                }
                let _ = writeln!(out, "  ]");
            }
            Term::Return(Some(v)) => {
                let _ = writeln!(out, "  ret {}", p.tv(*v));
            }
            Term::Return(None) => {
                let _ = writeln!(out, "  ret void");
            }
            Term::Unreachable => {
                let _ = writeln!(out, "  unreachable");
            }
        }
    }
    let _ = writeln!(out, "}}\n");
}

#[allow(clippy::too_many_lines)]
fn inst(out: &mut String, p: &mut Pr<'_>, own: &Sig, vars: &[Class], i: &Inst) {
    let m = p.m;
    match i {
        Inst::Param { dst, index } => {
            let c = own
                .params
                .get(*index as usize)
                .copied()
                .unwrap_or(Class::I64);
            p.alias(*dst, format!("%p{index}"), c);
        }
        Inst::Const { dst, class, value } => {
            let text = match class {
                Class::Bit => if *value != 0 { "true" } else { "false" }.to_owned(),
                Class::Ptr if *value == 0 => "null".to_owned(),
                Class::Ptr => format!("inttoptr (i64 {value} to ptr)"),
                _ => value.to_string(),
            };
            p.alias(*dst, text, *class);
        }
        Inst::Addr { dst, sym } => p.alias(*dst, name(m, *sym), Class::Ptr),
        Inst::SlotAddr { dst, slot } => p.alias(*dst, format!("%s{}", slot.0), Class::Ptr),
        Inst::Load {
            dst,
            class,
            addr,
            offset,
        } => {
            let a = p.at(out, *addr, *offset);
            let d = p.named(*dst, *class);
            let _ = writeln!(out, "  {d} = load {}, ptr {a}", ty(*class));
        }
        Inst::Store {
            addr,
            offset,
            value,
        } => {
            let a = p.at(out, *addr, *offset);
            let _ = writeln!(out, "  store {}, ptr {a}", p.tv(*value));
        }
        Inst::Bin { dst, op, a, b } => {
            let c = p.c(*a);
            let o = match op {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::SDiv => "sdiv",
                BinOp::SRem => "srem",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Xor => "xor",
            };
            let (x, y) = (p.v(*a), p.v(*b));
            let d = p.named(*dst, c);
            let _ = writeln!(out, "  {d} = {o} {} {x}, {y}", ty(c));
        }
        Inst::Ovf { dst, ovf, op, a, b } => {
            let o = match op {
                OvfOp::Add => "sadd",
                OvfOp::Sub => "ssub",
                OvfOp::Mul => "smul",
            };
            let t = p.temp();
            let _ = writeln!(
                out,
                "  {t} = call {{i64, i1}} @llvm.{o}.with.overflow.i64(i64 {}, i64 {})",
                p.v(*a),
                p.v(*b)
            );
            let d = p.named(*dst, Class::I64);
            let _ = writeln!(out, "  {d} = extractvalue {{i64, i1}} {t}, 0");
            let o = p.named(*ovf, Class::Bit);
            let _ = writeln!(out, "  {o} = extractvalue {{i64, i1}} {t}, 1");
        }
        Inst::Cmp { dst, op, a, b } => {
            let c = p.c(*a);
            let o = match op {
                Cmp::Eq => "eq",
                Cmp::Ne => "ne",
                Cmp::Lt => "slt",
                Cmp::Le => "sle",
                Cmp::Gt => "sgt",
                Cmp::Ge => "sge",
            };
            let (x, y) = (p.v(*a), p.v(*b));
            let d = p.named(*dst, Class::Bit);
            let _ = writeln!(out, "  {d} = icmp {o} {} {x}, {y}", ty(c));
        }
        Inst::Select { dst, cond, a, b } => {
            let c = p.c(*a);
            let (k, x, y) = (p.v(*cond), p.tv(*a), p.tv(*b));
            let d = p.named(*dst, c);
            let _ = writeln!(out, "  {d} = select i1 {k}, {x}, {y}");
        }
        Inst::Ext {
            dst,
            to,
            src,
            signed,
        } => {
            let from = p.c(*src);
            if from == *to {
                let s = p.v(*src);
                p.alias(*dst, s, *to);
            } else {
                let s = p.tv(*src);
                let d = p.named(*dst, *to);
                let o = if *signed { "sext" } else { "zext" };
                let _ = writeln!(out, "  {d} = {o} {s} to {}", ty(*to));
            }
        }
        Inst::Trunc { dst, to, src } => {
            let from = p.c(*src);
            if from == *to {
                let s = p.v(*src);
                p.alias(*dst, s, *to);
            } else {
                let s = p.tv(*src);
                let d = p.named(*dst, *to);
                let _ = writeln!(out, "  {d} = trunc {s} to {}", ty(*to));
            }
        }
        Inst::PtrAdd { dst, base, offset } => {
            let (b, o) = (p.v(*base), p.v(*offset));
            let d = p.named(*dst, Class::Ptr);
            let _ = writeln!(out, "  {d} = getelementptr i8, ptr {b}, i64 {o}");
        }
        Inst::PtrToInt { dst, src } => {
            let s = p.v(*src);
            let d = p.named(*dst, Class::I64);
            let _ = writeln!(out, "  {d} = ptrtoint ptr {s} to i64");
        }
        Inst::IntToPtr { dst, src } => {
            let s = p.v(*src);
            let d = p.named(*dst, Class::Ptr);
            let _ = writeln!(out, "  {d} = inttoptr i64 {s} to ptr");
        }
        Inst::GetVar { dst, var } => {
            let class = vars.get(var.0 as usize).copied().unwrap_or(Class::I64);
            let d = p.named(*dst, class);
            let _ = writeln!(out, "  {d} = load {}, ptr %x{}", ty(class), var.0);
        }
        Inst::SetVar { var, src } => {
            let _ = writeln!(out, "  store {}, ptr %x{}", p.tv(*src), var.0);
        }
        // The runtime's "has this thread failed?" is its thread-local flag: read in place, as the
        // runtime defines it, rather than called (instruction selection; the answer is the
        // entry's).
        Inst::Call {
            dst: Some(d),
            callee,
            ..
        } if m.symbol(*callee).is_some_and(|s| s.name == "nz.failed_now") => {
            let t = p.temp();
            let _ = writeln!(out, "  {t} = load i1, ptr @nz.failed");
            let d = p.named(*d, Class::I32);
            let _ = writeln!(out, "  {d} = zext i1 {t} to i32");
        }
        Inst::Call { dst, callee, args } => {
            let s = m.sig(*callee).cloned().unwrap_or(Sig {
                params: Vec::new(),
                ret: None,
                conv: Conv::Nazm,
            });
            let a = args
                .iter()
                .zip(s.params.iter().chain(std::iter::repeat(&Class::I64)))
                .map(|(v, c)| format!("{} {}", sig_ty(*c, s.conv, false), p.v(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            let callee = name(m, *callee);
            match (dst, s.ret) {
                (Some(d), Some(c)) => {
                    let d = p.named(*d, c);
                    let _ = writeln!(out, "  {d} = call {} {callee}({a})", ret_ty(&s));
                }
                _ => {
                    let _ = writeln!(out, "  call {} {callee}({a})", ret_ty(&s));
                }
            }
        }
        Inst::CallIndirect {
            dst,
            addr,
            sig,
            args,
        } => {
            let a = args
                .iter()
                .zip(sig.params.iter().chain(std::iter::repeat(&Class::I64)))
                .map(|(v, c)| format!("{} {}", sig_ty(*c, sig.conv, false), p.v(*v)))
                .collect::<Vec<_>>()
                .join(", ");
            let f = p.v(*addr);
            match (dst, sig.ret) {
                (Some(d), Some(c)) => {
                    let d = p.named(*d, c);
                    let _ = writeln!(out, "  {d} = call {} {f}({a})", ret_ty(sig));
                }
                _ => {
                    let _ = writeln!(out, "  call {} {f}({a})", ret_ty(sig));
                }
            }
        }
        Inst::Copy { dst, src, size } => {
            let _ = writeln!(
                out,
                "  call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {size}, i1 false)",
                p.v(*dst),
                p.v(*src)
            );
        }
        Inst::Zero { dst, size } => {
            let _ = writeln!(
                out,
                "  call void @llvm.memset.p0.i64(ptr {}, i8 0, i64 {size}, i1 false)",
                p.v(*dst)
            );
        }
        Inst::Loc(pos) => {
            let _ = writeln!(out, "  ; nz.loc {} {} {}", pos.file, pos.line, pos.column);
        }
    }
}
