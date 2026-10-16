//! LIR as LLVM IR text (N105, `docs/architecture.md` §7.106): the LLVM backend's whole job.
//!
//! Each LIR instruction is the LLVM instruction it names; each variable and each slot an
//! `alloca` in the entry block, which LLVM's own promotion turns into registers. Nothing here
//! decides anything a program can observe.
//!
//! How a failure crosses a call is the target's (4.2, `docs/architecture.md` §7.115): the failure
//! bit returned beside the result ([`Failures::Bit`]), or a forced unwind ([`Failures::Unwind`]).
//! LIR is the same for both.

use std::fmt::Write as _;

use super::ir::{
    BinOp, Class, Cmp, Conv, ConvOp, Function, Inst, Linkage, Module, OvfOp, Sig, Storage, SymId,
    SymKind, Term, UnOp, V,
};

const fn ty(c: Class) -> &'static str {
    match c {
        Class::Bit => "i1",
        Class::I8 => "i8",
        Class::I16 => "i16",
        Class::I32 => "i32",
        Class::I64 => "i64",
        Class::F32 => "float",
        Class::F64 => "double",
        Class::Ptr => "ptr",
        Class::I128 => "i128",
    }
}

/// An `I128`'s explicit alignment: the layout gives it 8 (`abi.rs`), less than LLVM's own, so
/// every load and store of one says so (R1b, L13b).
const fn align(c: Class) -> &'static str {
    match c {
        Class::I128 => ", align 8",
        _ => "",
    }
}

/// A float constant as LLVM writes one exactly: the hexadecimal of its value as a `double`,
/// which a `Float32` value always has exactly (Gate 2).
fn float_text(class: Class, bits: i64) -> String {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let as_double = if class == Class::F32 {
        f64::from(f32::from_bits(bits as u32)).to_bits()
    } else {
        bits as u64
    };
    format!("0x{as_double:016X}")
}

/// A parameter's or result's type in a signature: C's `bool` is zero-extended (N42).
fn sig_ty(c: Class, conv: Conv, result: bool) -> String {
    match (c, conv, result) {
        (Class::Bit, Conv::C, true) => "zeroext i1".to_owned(),
        (Class::Bit, Conv::C, false) => "i1 zeroext".to_owned(),
        _ => ty(c).to_owned(),
    }
}

/// How a Nazm function's failure reaches its caller on a target (4.2, `docs/architecture.md`
/// §7.115). A program cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failures {
    /// The failure bit, returned beside the result (R1, 5b.8): WebAssembly, WASI, the boards
    /// and Windows, which have no unwinder this compiler uses.
    Bit,
    /// A forced unwind (`nz.raise`, `_Unwind_ForcedUnwind`) under the C personality,
    /// `__gcc_personality_v0`: the hosted targets. A Nazm function returns its result alone.
    Unwind,
}

/// How a generated function probes a frame larger than a page on a target (Q1-A1-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probes {
    /// LLVM's own probe loop, inline (`"probe-stack"="inline-asm"`): every target but Windows.
    Inline,
    /// The platform's probe, which LLVM calls before every large frame unasked: Windows's
    /// `__chkstk`, MinGW-w64's `___chkstk_ms`, which also touches a thread's stack page by page
    /// as Windows grows it. LLVM never probes inline there: given the attribute, it calls a
    /// function named `inline-asm`, which no linker finds.
    Platform,
}

impl Probes {
    /// What a function's definition says about its probes, with its leading space, or nothing.
    const fn attribute(self) -> &'static str {
        match self {
            Self::Inline => " \"probe-stack\"=\"inline-asm\"",
            Self::Platform => "",
        }
    }
}

/// A function's LLVM result: a Nazm function's failure bit beside its result, as a first-class
/// pair a target returns in two registers (R1, 5b.8), or alone where there is no result. Where
/// failures unwind, the result alone.
fn ret_ty(s: &Sig, failures: Failures) -> String {
    match (s.ret, s.fails() && failures == Failures::Bit) {
        (Some(c), true) => format!("{{{}, i1}}", ty(c)),
        (None, true) => "i1".to_owned(),
        (Some(c), false) => sig_ty(c, s.conv, true),
        (None, false) => "void".to_owned(),
    }
}

/// Where failures unwind: the exception object, one per thread; the stop function, which lets
/// every frame's cleanup run and aborts only at the end of the stack — never reached, since every
/// plain entry lands the unwind; and `nz.raise`, which a failure exit calls. Phase 2 only: no
/// search, no catch, no type information. Defined once per image whichever units define them.
const RAISE: &str = r#"declare i32 @_Unwind_ForcedUnwind(ptr, ptr, ptr)
declare void @abort() noreturn
@nz.exc = linkonce_odr hidden thread_local global [4 x i64] zeroinitializer, align 16
define linkonce_odr hidden i32 @nz.unwind_stop(i32 %v, i32 %a, i64 %c, ptr %e, ptr %x, ptr %p) "probe-stack"="inline-asm" {
  %end = and i32 %a, 16
  %at = icmp ne i32 %end, 0
  br i1 %at, label %over, label %on
over:
  call void @abort()
  unreachable
on:
  ret i32 0
}
define linkonce_odr hidden void @nz.raise() noinline cold "probe-stack"="inline-asm" {
  store i64 5638864641536806221, ptr @nz.exc
  %r = call i32 @_Unwind_ForcedUnwind(ptr @nz.exc, ptr @nz.unwind_stop, ptr null)
  call void @abort()
  unreachable
}"#;

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
pub fn module(m: &Module, failures: Failures, probes: Probes) -> String {
    let mut out = String::new();
    crate::runtime::preamble(&mut out, &m.source);
    // Every width's overflow-checked operations, bit counts and rotations, and the float
    // operations LLVM has exactly (Gate 2). An unused declaration costs nothing.
    for w in ["i8", "i16", "i32", "i64"] {
        for o in ["sadd", "ssub", "smul", "uadd", "usub", "umul"] {
            let _ = writeln!(
                out,
                "declare {{{w}, i1}} @llvm.{o}.with.overflow.{w}({w}, {w})"
            );
        }
        let _ = writeln!(out, "declare {w} @llvm.ctpop.{w}({w})");
        for (f, t) in [("f32", "float"), ("f64", "double")] {
            for o in ["fptosi", "fptoui"] {
                let _ = writeln!(out, "declare {w} @llvm.{o}.sat.{w}.{f}({t})");
            }
        }
        let _ = writeln!(out, "declare {w} @llvm.ctlz.{w}({w}, i1)");
        let _ = writeln!(out, "declare {w} @llvm.cttz.{w}({w}, i1)");
        let _ = writeln!(out, "declare {w} @llvm.fshl.{w}({w}, {w}, {w})");
        let _ = writeln!(out, "declare {w} @llvm.fshr.{w}({w}, {w}, {w})");
        if w != "i8" {
            let _ = writeln!(out, "declare {w} @llvm.bswap.{w}({w})");
        }
    }
    for (f, t) in [("f32", "float"), ("f64", "double")] {
        for o in ["sqrt", "fabs", "floor", "ceil", "trunc"] {
            let _ = writeln!(out, "declare {t} @llvm.{o}.{f}({t})");
        }
    }
    let _ = writeln!(
        out,
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)"
    );
    let _ = writeln!(out, "declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)");
    if failures == Failures::Unwind {
        let _ = writeln!(out, "declare i32 @__gcc_personality_v0(...)");
        if m.functions
            .iter()
            .any(|f| f.blocks.iter().any(|b| b.term == Term::Fail))
        {
            let _ = writeln!(out, "{RAISE}");
        }
    }
    if m.symbols.iter().any(|s| s.name == "nz.failed_now") {
        let _ = writeln!(out, "@nz.failed = external hidden thread_local global i1");
    }
    // The stack check, inlined (the speed milestone): the limit is read here and the runtime
    // called only when it is unset or crossed.
    if m.symbols.iter().any(|s| s.name == "nz.stack_check") {
        let _ = writeln!(
            out,
            "@nz.stack_limit = external hidden thread_local global i64"
        );
        let _ = writeln!(out, "declare ptr @llvm.frameaddress.p0(i32)");
    }
    // The preemption poll, inlined (6.3): the flag is read here and the runtime called only when
    // the pool has asked some task to yield.
    if m.symbols.iter().any(|s| s.name == "nz.preempt") {
        let _ = writeln!(out, "@nz.preempt_req = external hidden global i64");
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
                let _ = writeln!(
                    out,
                    "declare {} {}({params})",
                    ret_ty(sig, failures),
                    name(m, id)
                );
            }
            SymKind::Data { class } => {
                // The runtime's state is `hidden` in the image this unit is linked into, so it is
                // addressed directly rather than through the global offset table (the speed
                // milestone); a C library's data keeps default visibility.
                let vis = if s.name.starts_with("nz.") {
                    "hidden "
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "{} = external {vis}global {}",
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
        // A static (Gate 3) is its module's, under its own name, and a static atomic's cell is
        // written as the program runs; every other datum is a private literal.
        let local = m.symbol(d.sym).is_none_or(|s| s.linkage == Linkage::Local);
        let place = if local {
            "private unnamed_addr "
        } else {
            "hidden "
        };
        let kind = if d.writable { "global" } else { "constant" };
        let section = d
            .section
            .as_ref()
            .map_or_else(String::new, |s| format!(", section \"{s}\""));
        let _ = writeln!(
            out,
            "{} = {place}{kind} {body}{section}, align {}",
            name(m, d.sym),
            d.align.max(1)
        );
    }
    out.push('\n');
    for f in &m.functions {
        function(&mut out, m, f, failures, probes);
    }
    out
}

struct Pr<'m> {
    m: &'m Module,
    vals: Vec<Option<String>>,
    classes: Vec<Class>,
    temp: u32,
    failures: Failures,
    /// Where failures unwind, the call being printed fails to a bare failure exit: a plain call,
    /// which an unwind passes through, with no landing pad (4.2).
    bare: bool,
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

/// Whether block `bi` ends in a Nazm call whose failure edge is a bare failure exit — a block that
/// only fails, releasing nothing (4.2): where failures unwind, that call needs no landing pad.
fn bare_edge(f: &Function, bi: usize) -> bool {
    let Some(b) = f.blocks.get(bi) else {
        return false;
    };
    let failed = match b.insts.last() {
        Some(
            Inst::Call {
                failed: Some(fv), ..
            }
            | Inst::CallIndirect {
                failed: Some(fv), ..
            },
        ) => *fv,
        _ => return false,
    };
    match &b.term {
        Term::Branch { cond, then, .. } if *cond == failed => {
            f.blocks.get(then.index()).is_some_and(|t| {
                t.term == Term::Fail && t.insts.iter().all(|i| matches!(i, Inst::Loc(_)))
            })
        }
        _ => false,
    }
}

/// Whether `i` calls a Nazm function: where failures unwind, a call that may unwind.
fn nazm_call(m: &Module, i: &Inst) -> bool {
    match i {
        Inst::Call { callee, .. } => m.sig(*callee).is_some_and(Sig::fails),
        Inst::CallIndirect { sig, .. } => sig.fails(),
        _ => false,
    }
}

#[allow(clippy::too_many_lines)]
fn function(out: &mut String, m: &Module, f: &Function, failures: Failures, probes: Probes) {
    let Some(sym) = m.symbol(f.sym) else { return };
    let Some(sig) = m.sig(f.sym) else { return };
    let params = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} %p{i}", sig_ty(*c, sig.conv, false)))
        .collect::<Vec<_>>()
        .join(", ");
    // Stack probes on every generated function (Q1-A1-01): a frame larger than the one guard
    // page below a task's stack touches each page on its way down, so an overrun faults on the
    // guard instead of landing in another task's stack or the heap. On Windows the platform's
    // probe does it, unasked ([`Probes::Platform`]).
    // A function `@section` places (Gate 3) names its section; a linker script keeps it.
    let section = f
        .section
        .as_ref()
        .map_or_else(String::new, |s| format!(" section \"{s}\""));
    // Where failures unwind, a function with a landing pad names the C personality (4.2): one
    // that calls a Nazm function other than straight into a bare failure exit.
    let unwind = failures == Failures::Unwind;
    let bare: Vec<bool> = (0..f.blocks.len())
        .map(|bi| unwind && bare_edge(f, bi))
        .collect();
    let pads = unwind
        && f.blocks.iter().enumerate().any(|(bi, b)| {
            let n = b.insts.len();
            b.insts.iter().enumerate().any(|(k, i)| {
                nazm_call(m, i) && !(k + 1 == n && bare.get(bi).copied().unwrap_or(false))
            })
        });
    let personality = if pads {
        " personality ptr @__gcc_personality_v0"
    } else {
        ""
    };
    let probe = probes.attribute();
    let _ = writeln!(
        out,
        "define {}{} {}({params}){probe}{section}{personality} {{   ; {}",
        linkage(sym.linkage),
        ret_ty(sig, failures),
        name(m, f.sym),
        f.name
    );
    let mut p = Pr {
        m,
        vals: vec![None; f.values as usize],
        classes: vec![Class::I64; f.values as usize],
        temp: 0,
        failures,
        bare: false,
    };
    let cold = f.cold_blocks(m);
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
        let last = b.insts.len();
        for (k, i) in b.insts.iter().enumerate() {
            p.bare = k + 1 == last && bare.get(bi).copied().unwrap_or(false);
            inst(out, &mut p, sig, &f.vars, i);
        }
        p.bare = false;
        match &b.term {
            Term::Jump(t) => {
                let _ = writeln!(out, "  br label %{}", label(t.index()));
            }
            Term::Branch {
                cond,
                then,
                otherwise,
            } => {
                // A branch to where only a failure goes is weighted away from it, so the code that
                // does not fail is laid out straight (R1, 5b.8).
                let is = |b: &super::ir::B| cold.get(b.index()).copied().unwrap_or(false);
                let weights = match (is(then), is(otherwise)) {
                    (true, false) => ", !prof !{!\"branch_weights\", i32 1, i32 2000}",
                    (false, true) => ", !prof !{!\"branch_weights\", i32 2000, i32 1}",
                    _ => "",
                };
                let _ = writeln!(
                    out,
                    "  br i1 {}, label %{}, label %{}{weights}",
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
            Term::Return(Some(v)) if sig.fails() && !unwind => {
                let r = ret_ty(sig, failures);
                let (a, b) = (p.temp(), p.temp());
                let _ = writeln!(out, "  {a} = insertvalue {r} undef, {}, 0", p.tv(*v));
                let _ = writeln!(out, "  {b} = insertvalue {r} {a}, i1 false, 1");
                let _ = writeln!(out, "  ret {r} {b}");
            }
            Term::Return(None) if sig.fails() && !unwind => {
                let _ = writeln!(out, "  ret i1 false");
            }
            Term::Return(Some(v)) => {
                let _ = writeln!(out, "  ret {}", p.tv(*v));
            }
            Term::Return(None) => {
                let _ = writeln!(out, "  ret void");
            }
            // The failure exit, where failures unwind: to the nearest frame with a landing pad.
            Term::Fail if unwind => {
                let _ = writeln!(out, "  call void @nz.raise()");
                let _ = writeln!(out, "  unreachable");
            }
            // The failure exit: the bit set, and a zero where the result would be.
            Term::Fail => {
                let _ = match sig.ret {
                    Some(c) => writeln!(
                        out,
                        "  ret {} {{ {} zeroinitializer, i1 true }}",
                        ret_ty(sig, failures),
                        ty(c)
                    ),
                    None => writeln!(out, "  ret i1 true"),
                };
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
                // A float constant is its bits, at most 64 of them.
                #[allow(clippy::cast_possible_truncation)]
                Class::F32 | Class::F64 => float_text(*class, *value as i64),
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
            let _ = writeln!(out, "  {d} = load {}, ptr {a}{}", ty(*class), align(*class));
        }
        Inst::Store {
            addr,
            offset,
            value,
        } => {
            let a = p.at(out, *addr, *offset);
            let _ = writeln!(
                out,
                "  store {}, ptr {a}{}",
                p.tv(*value),
                align(p.c(*value))
            );
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
                BinOp::UDiv => "udiv",
                BinOp::URem => "urem",
                BinOp::Shl => "shl",
                BinOp::LShr => "lshr",
                BinOp::AShr => "ashr",
                BinOp::FAdd => "fadd",
                BinOp::FSub => "fsub",
                BinOp::FMul => "fmul",
                BinOp::FDiv => "fdiv",
                BinOp::Rotl | BinOp::Rotr => {
                    let f = if *op == BinOp::Rotl { "fshl" } else { "fshr" };
                    let (x, y) = (p.v(*a), p.v(*b));
                    let d = p.named(*dst, c);
                    let t = ty(c);
                    let _ = writeln!(
                        out,
                        "  {d} = call {t} @llvm.{f}.{t}({t} {x}, {t} {x}, {t} {y})"
                    );
                    return;
                }
            };
            let (x, y) = (p.v(*a), p.v(*b));
            let d = p.named(*dst, c);
            let _ = writeln!(out, "  {d} = {o} {} {x}, {y}", ty(c));
        }
        Inst::Ovf { dst, ovf, op, a, b } => {
            let c = p.c(*a);
            let w = ty(c);
            let o = match op {
                OvfOp::Add => "sadd",
                OvfOp::Sub => "ssub",
                OvfOp::Mul => "smul",
                OvfOp::UAdd => "uadd",
                OvfOp::USub => "usub",
                OvfOp::UMul => "umul",
            };
            let t = p.temp();
            let _ = writeln!(
                out,
                "  {t} = call {{{w}, i1}} @llvm.{o}.with.overflow.{w}({w} {}, {w} {})",
                p.v(*a),
                p.v(*b)
            );
            let d = p.named(*dst, c);
            let _ = writeln!(out, "  {d} = extractvalue {{{w}, i1}} {t}, 0");
            let o = p.named(*ovf, Class::Bit);
            let _ = writeln!(out, "  {o} = extractvalue {{{w}, i1}} {t}, 1");
        }
        Inst::Un { dst, op, a } => {
            let c = p.c(*a);
            let t = ty(c);
            let x = p.v(*a);
            let f = if c == Class::F32 { "f32" } else { "f64" };
            let d = p.named(*dst, c);
            let _ = match op {
                UnOp::FNeg => writeln!(out, "  {d} = fneg {t} {x}"),
                UnOp::FSqrt => writeln!(out, "  {d} = call {t} @llvm.sqrt.{f}({t} {x})"),
                UnOp::FAbs => writeln!(out, "  {d} = call {t} @llvm.fabs.{f}({t} {x})"),
                UnOp::FFloor => writeln!(out, "  {d} = call {t} @llvm.floor.{f}({t} {x})"),
                UnOp::FCeil => writeln!(out, "  {d} = call {t} @llvm.ceil.{f}({t} {x})"),
                UnOp::FTrunc => writeln!(out, "  {d} = call {t} @llvm.trunc.{f}({t} {x})"),
                UnOp::Popcnt => writeln!(out, "  {d} = call {t} @llvm.ctpop.{t}({t} {x})"),
                UnOp::Clz => writeln!(out, "  {d} = call {t} @llvm.ctlz.{t}({t} {x}, i1 false)"),
                UnOp::Ctz => writeln!(out, "  {d} = call {t} @llvm.cttz.{t}({t} {x}, i1 false)"),
                UnOp::Bswap if c == Class::I8 => writeln!(out, "  {d} = or i8 {x}, 0"),
                UnOp::Bswap => writeln!(out, "  {d} = call {t} @llvm.bswap.{t}({t} {x})"),
            };
        }
        // Float to integer saturates, as Cranelift's does: the lowering has range-checked an exact
        // conversion and wants saturation from the other, so neither ever meets LLVM's poison.
        Inst::Conv {
            dst,
            op: op @ (ConvOp::FToS | ConvOp::FToU),
            to,
            src,
        } => {
            let from = p.c(*src);
            let (f, ft) = if from == Class::F32 {
                ("f32", "float")
            } else {
                ("f64", "double")
            };
            let s = p.v(*src);
            let t = ty(*to);
            let o = if *op == ConvOp::FToS {
                "fptosi"
            } else {
                "fptoui"
            };
            let d = p.named(*dst, *to);
            let _ = writeln!(out, "  {d} = call {t} @llvm.{o}.sat.{t}.{f}({ft} {s})");
        }
        Inst::Conv { dst, op, to, src } => {
            let s = p.tv(*src);
            let d = p.named(*dst, *to);
            let o = match op {
                ConvOp::SToF => "sitofp",
                ConvOp::UToF => "uitofp",
                ConvOp::FToS => "fptosi",
                ConvOp::FToU => "fptoui",
                ConvOp::FExt => "fpext",
                ConvOp::FTrunc => "fptrunc",
                ConvOp::Bits => "bitcast",
            };
            let _ = writeln!(out, "  {d} = {o} {s} to {}", ty(*to));
        }
        Inst::Cmp { dst, op, a, b } => {
            let c = p.c(*a);
            let (inst, o) = match op {
                Cmp::Eq => ("icmp", "eq"),
                Cmp::Ne => ("icmp", "ne"),
                Cmp::Lt => ("icmp", "slt"),
                Cmp::Le => ("icmp", "sle"),
                Cmp::Gt => ("icmp", "sgt"),
                Cmp::Ge => ("icmp", "sge"),
                Cmp::ULt => ("icmp", "ult"),
                Cmp::ULe => ("icmp", "ule"),
                Cmp::UGt => ("icmp", "ugt"),
                Cmp::UGe => ("icmp", "uge"),
                // Ordered, false with a NaN; `!=` unordered, true with one: IEEE 754's.
                Cmp::FEq => ("fcmp", "oeq"),
                Cmp::FNe => ("fcmp", "une"),
                Cmp::FLt => ("fcmp", "olt"),
                Cmp::FLe => ("fcmp", "ole"),
                Cmp::FGt => ("fcmp", "ogt"),
                Cmp::FGe => ("fcmp", "oge"),
            };
            let (x, y) = (p.v(*a), p.v(*b));
            let d = p.named(*dst, Class::Bit);
            let _ = writeln!(out, "  {d} = {inst} {o} {} {x}, {y}", ty(c));
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
        // The stack check (N49), inlined: this frame against the thread's limit, which the runtime
        // sets on the thread's first check. Only then — the limit 0 — or past it is the runtime
        // called, and its answer is the check's; otherwise the answer is 0. One load and two
        // comparisons where every recursive call was a call into another module.
        // The preemption poll at a loop's back edge (6.3), inlined: one load of a flag no thread
        // writes until the pool asks a task to yield, and a branch weighted past the call.
        Inst::Call {
            dst: None,
            callee,
            args,
            ..
        } if args.is_empty() && m.symbol(*callee).is_some_and(|s| s.name == "nz.preempt") => {
            let (flag, asked) = (p.temp(), p.temp());
            let n = p.temp();
            let n = n.trim_start_matches('%');
            let _ = writeln!(
                out,
                "  {flag} = load atomic i64, ptr @nz.preempt_req monotonic, align 8"
            );
            let _ = writeln!(out, "  {asked} = icmp ne i64 {flag}, 0");
            let _ = writeln!(
                out,
                "  br i1 {asked}, label %pp.slow.{n}, label %pp.on.{n}, !prof !{{!\"branch_weights\", i32 1, i32 2000}}"
            );
            let _ = writeln!(out, "pp.slow.{n}:");
            let _ = writeln!(out, "  call void @nz.preempt()");
            let _ = writeln!(out, "  br label %pp.on.{n}");
            let _ = writeln!(out, "pp.on.{n}:");
        }
        Inst::Call {
            dst: Some(d),
            callee,
            args,
            ..
        } if args.is_empty()
            && m.symbol(*callee)
                .is_some_and(|s| s.name == "nz.stack_check") =>
        {
            let (fp, sp, lim, past, unset, slow) =
                (p.temp(), p.temp(), p.temp(), p.temp(), p.temp(), p.temp());
            let n = p.temp();
            let n = n.trim_start_matches('%');
            let r = p.temp();
            let _ = writeln!(out, "  {fp} = call ptr @llvm.frameaddress.p0(i32 0)");
            let _ = writeln!(out, "  {sp} = ptrtoint ptr {fp} to i64");
            let _ = writeln!(out, "  {lim} = load i64, ptr @nz.stack_limit");
            let _ = writeln!(out, "  {past} = icmp ult i64 {sp}, {lim}");
            let _ = writeln!(out, "  {unset} = icmp eq i64 {lim}, 0");
            let _ = writeln!(out, "  {slow} = or i1 {past}, {unset}");
            // Weighted, so the path that does not call the runtime falls through (4.2): with the
            // failure bit gone, this is half of what a recursive call gains.
            let _ = writeln!(
                out,
                "  br i1 {slow}, label %sc.slow.{n}, label %sc.fast.{n}, !prof !{{!\"branch_weights\", i32 1, i32 2000}}"
            );
            let _ = writeln!(out, "sc.slow.{n}:");
            let _ = writeln!(out, "  {r} = call i32 @nz.stack_check()");
            let _ = writeln!(out, "  br label %sc.join.{n}");
            let _ = writeln!(out, "sc.fast.{n}:");
            let _ = writeln!(out, "  br label %sc.join.{n}");
            let _ = writeln!(out, "sc.join.{n}:");
            let d = p.named(*d, Class::I32);
            let _ = writeln!(
                out,
                "  {d} = phi i32 [ {r}, %sc.slow.{n} ], [ 0, %sc.fast.{n} ]"
            );
        }
        Inst::Call {
            dst,
            failed,
            callee,
            args,
        } => {
            let s = m.sig(*callee).cloned().unwrap_or(Sig {
                params: Vec::new(),
                ret: None,
                conv: Conv::Plain,
            });
            let callee = name(m, *callee);
            call(out, p, &s, &callee, args, *dst, *failed);
        }
        Inst::CallIndirect {
            dst,
            failed,
            addr,
            sig,
            args,
        } => {
            let f = p.v(*addr);
            call(out, p, sig, &f, args, *dst, *failed);
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

/// A call, direct or not: its result, and a Nazm callee's failure bit, each taken out of the pair
/// it returns (R1, 5b.8) — or, where failures unwind, the unwind's edge (4.2).
fn call(
    out: &mut String,
    p: &mut Pr<'_>,
    s: &Sig,
    callee: &str,
    args: &[V],
    dst: Option<V>,
    failed: Option<V>,
) {
    let a = args
        .iter()
        .zip(s.params.iter().chain(std::iter::repeat(&Class::I64)))
        .map(|(v, c)| format!("{} {}", sig_ty(*c, s.conv, false), p.v(*v)))
        .collect::<Vec<_>>()
        .join(", ");
    let r = ret_ty(s, p.failures);
    if s.fails() && p.failures == Failures::Unwind {
        unwinding_call(out, p, s, &format!("{r} {callee}({a})"), dst, failed);
        return;
    }
    match (s.ret, s.fails()) {
        (Some(c), true) => {
            let t = p.temp();
            let _ = writeln!(out, "  {t} = call {r} {callee}({a})");
            if let Some(d) = dst {
                let d = p.named(d, c);
                let _ = writeln!(out, "  {d} = extractvalue {r} {t}, 0");
            }
            if let Some(f) = failed {
                let f = p.named(f, Class::Bit);
                let _ = writeln!(out, "  {f} = extractvalue {r} {t}, 1");
            }
        }
        (None, true) => match failed {
            Some(f) => {
                let f = p.named(f, Class::Bit);
                let _ = writeln!(out, "  {f} = call {r} {callee}({a})");
            }
            None => {
                let _ = writeln!(out, "  call {r} {callee}({a})");
            }
        },
        (Some(c), false) if dst.is_some() => {
            if let Some(d) = dst {
                let d = p.named(d, c);
                let _ = writeln!(out, "  {d} = call {r} {callee}({a})");
            }
        }
        _ => {
            let _ = writeln!(out, "  call {r} {callee}({a})");
        }
    }
}

/// A Nazm call where failures unwind (4.2, `docs/architecture.md` §7.115). Straight into a bare
/// failure exit it is a plain call, which an unwind passes through, and its failure edge is never
/// taken here. Otherwise an `invoke` whose cleanup landing pad joins the normal path with the edge
/// taken — the LIR's own branch then goes where the failure goes — or, from a call with no failure
/// edge (a plain entry's call of its body), as if the call had returned, the result a zero: the
/// caller reads the flag, and C is never unwound into.
fn unwinding_call(
    out: &mut String,
    p: &mut Pr<'_>,
    s: &Sig,
    call: &str,
    dst: Option<V>,
    failed: Option<V>,
) {
    let named = match (s.ret, dst) {
        (Some(c), Some(d)) => Some((p.named(d, c), c)),
        _ => None,
    };
    let plain = p.bare && failed.is_some();
    if plain {
        if let Some(fv) = failed {
            p.alias(fv, "false".to_owned(), Class::Bit);
        }
        let _ = match &named {
            Some((d, _)) => writeln!(out, "  {d} = call {call}"),
            None => writeln!(out, "  call {call}"),
        };
        return;
    }
    p.temp += 1;
    let n = p.temp;
    let _ = writeln!(out, "  br label %iv.{n}");
    let _ = writeln!(out, "iv.{n}:");
    let t = format!("%iv.{n}.r");
    let _ = match &named {
        Some(_) => writeln!(
            out,
            "  {t} = invoke {call} to label %ok.{n} unwind label %lp.{n}"
        ),
        None => writeln!(out, "  invoke {call} to label %ok.{n} unwind label %lp.{n}"),
    };
    let _ = writeln!(out, "lp.{n}:");
    let _ = writeln!(out, "  %lp.{n}.x = landingpad {{ ptr, i32 }} cleanup");
    let _ = writeln!(out, "  br label %ok.{n}");
    let _ = writeln!(out, "ok.{n}:");
    if let Some((d, c)) = named {
        let stand_in = if failed.is_some() {
            "poison"
        } else {
            "zeroinitializer"
        };
        let _ = writeln!(
            out,
            "  {d} = phi {} [ {t}, %iv.{n} ], [ {stand_in}, %lp.{n} ]",
            ty(c)
        );
    }
    if let Some(fv) = failed {
        let f = p.named(fv, Class::Bit);
        let _ = writeln!(out, "  {f} = phi i1 [ false, %iv.{n} ], [ true, %lp.{n} ]");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::expect_used)]

    use super::{module, Failures, Probes};
    use crate::op::ir::{
        Block, Class, Conv, Function, Inst, Linkage, Module, Sig, SymId, SymKind, Symbol, Term, B,
        V,
    };

    fn func(name: &str, conv: Conv, linkage: Linkage) -> Symbol {
        Symbol {
            name: name.to_owned(),
            linkage,
            kind: SymKind::Func {
                sig: Sig {
                    params: vec![Class::I64],
                    ret: Some(Class::I64),
                    conv,
                },
            },
        }
    }

    fn function(sym: u32, name: &str, blocks: Vec<Block>) -> Function {
        Function {
            sym: SymId(sym),
            name: name.to_owned(),
            vars: Vec::new(),
            slots: Vec::new(),
            var_names: Vec::new(),
            slot_names: Vec::new(),
            blocks,
            values: 8,
            at: None,
            debug_vars: Vec::new(),
            section: None,
        }
    }

    fn call(arg: V, dst: u32, failed: Option<u32>) -> Inst {
        Inst::Call {
            dst: Some(V(dst)),
            failed: failed.map(V),
            callee: SymId(1),
            args: vec![arg],
        }
    }

    /// `f` calls the Nazm `g` twice: the first call's failure edge is a bare `fail`, the second's
    /// does something first. `h`, a plain entry, calls `f` with no failure edge.
    fn calls() -> Module {
        let param = Inst::Param {
            dst: V(0),
            index: 0,
        };
        let branch = |cond: u32, then: u32, otherwise: u32| Term::Branch {
            cond: V(cond),
            then: B(then),
            otherwise: B(otherwise),
        };
        let f = function(
            0,
            "f",
            vec![
                Block {
                    insts: vec![param.clone(), call(V(0), 1, Some(2))],
                    term: branch(2, 1, 2),
                },
                Block {
                    insts: Vec::new(),
                    term: Term::Fail,
                },
                Block {
                    insts: vec![call(V(1), 3, Some(4))],
                    term: branch(4, 3, 4),
                },
                Block {
                    insts: vec![Inst::Const {
                        dst: V(5),
                        class: Class::I64,
                        value: 0,
                    }],
                    term: Term::Fail,
                },
                Block {
                    insts: Vec::new(),
                    term: Term::Return(Some(V(3))),
                },
            ],
        );
        let mut entry = call(V(0), 1, None);
        if let Inst::Call { callee, .. } = &mut entry {
            *callee = SymId(0);
        }
        let h = function(
            2,
            "h",
            vec![Block {
                insts: vec![param, entry],
                term: Term::Return(Some(V(1))),
            }],
        );
        Module {
            source: "t.nz".to_owned(),
            pointer: 8,
            symbols: vec![
                func("f", Conv::Nazm, Linkage::Local),
                func("g", Conv::Nazm, Linkage::Import),
                func("h", Conv::Plain, Linkage::Hidden),
            ],
            functions: vec![f, h],
            data: Vec::new(),
            needs: crate::Needs::default(),
        }
    }

    #[test]
    fn a_failure_is_the_bit_or_an_unwind_by_the_target() {
        let m = calls();
        assert!(
            crate::op::validate::module(&m).is_ok(),
            "{:?}",
            crate::op::validate::module(&m)
        );
        // The bit (WebAssembly, WASI, the boards, Windows): the pair, and the exit that sets it.
        let bit = module(&m, Failures::Bit, Probes::Inline);
        assert!(
            bit.contains("define internal {i64, i1} @f(i64 %p0)"),
            "{bit}"
        );
        assert!(bit.contains("declare {i64, i1} @g(i64)"), "{bit}");
        assert!(
            bit.contains("  ret {i64, i1} { i64 zeroinitializer, i1 true }"),
            "{bit}"
        );
        assert!(
            !bit.contains("invoke") && !bit.contains("nz.raise"),
            "{bit}"
        );
        // Unwinding (the hosted targets, 4.2): the result alone; a call into a bare failure exit
        // is a plain call, one whose edge does something an invoke with a landing pad, the plain
        // entry's an invoke whose pad continues with a zero, and a failure exit raises.
        let unwind = module(&m, Failures::Unwind, Probes::Inline);
        assert!(unwind.contains("declare i64 @g(i64)"), "{unwind}");
        assert!(
            unwind.contains(
                "define internal i64 @f(i64 %p0) \"probe-stack\"=\"inline-asm\" personality ptr @__gcc_personality_v0"
            ),
            "{unwind}"
        );
        assert!(
            unwind.contains("  %v1 = call i64 @g(i64 %p0)\n"),
            "{unwind}"
        );
        assert!(
            unwind.contains("= invoke i64 @g(i64 %v1) to label"),
            "{unwind}"
        );
        assert!(
            unwind.contains("landingpad { ptr, i32 } cleanup"),
            "{unwind}"
        );
        assert!(unwind.contains("%v4 = phi i1 [ false, %iv."), "{unwind}");
        assert!(
            unwind.contains("= invoke i64 @f(i64 %p0) to label"),
            "{unwind}"
        );
        assert!(unwind.contains("[ zeroinitializer, %lp."), "{unwind}");
        assert_eq!(
            unwind
                .matches("  call void @nz.raise()\n  unreachable")
                .count(),
            2
        );
        assert!(
            !unwind.contains(", i1} @f") && !unwind.contains(", i1} @g"),
            "{unwind}"
        );
    }

    /// Q1-A1-01 on Windows: every definition probes inline, except where the platform's probe
    /// is the one LLVM calls — there no definition names the inline probe.
    #[test]
    fn a_definition_probes_inline_or_leaves_it_to_the_platform() {
        let m = calls();
        let inline = module(&m, Failures::Bit, Probes::Inline);
        let platform = module(&m, Failures::Bit, Probes::Platform);
        for (text, attribute) in [(&inline, true), (&platform, false)] {
            let defines: Vec<&str> = text.lines().filter(|l| l.starts_with("define ")).collect();
            assert!(!defines.is_empty(), "{text}");
            for d in defines {
                assert_eq!(
                    d.contains("\"probe-stack\"=\"inline-asm\""),
                    attribute,
                    "{d}"
                );
            }
        }
        assert!(!platform.contains("inline-asm"), "{platform}");
    }
}
