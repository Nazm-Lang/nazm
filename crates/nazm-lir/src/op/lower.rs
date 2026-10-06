//! Validated MIR to LIR, one unit at a time: the one place a native operation is decided
//! (N105, `docs/architecture.md` §7.106).
//!
//! Every guard and its failure text, every runtime entry an operation calls, every retain and
//! release and the helpers that do them, every offset a value is read or written at, and the
//! calling convention, are written here as LIR instructions. A backend translates them.

use std::collections::BTreeMap;

use nazm_diag::{codes, Diagnostic};
use nazm_mir as mir;
use nazm_span::Span;

use super::ir::{
    Block, Class, Conv, Data, DebugVar, Function, Linkage, Module, Pos, Sig, SlotDecl, SymId,
    SymKind, Symbol, Term, Var, B, V,
};
use crate::abi::{ByteLayouts, DataLayout};
use crate::ir::{EnumTy, RecordTy, Ty, Unit};
use crate::lower::Native;
use crate::names::SourceNames;
use crate::runtime::{self, Part, Word};
use crate::Needs;

/// The runtime's own layouts (`nazm-runtime`): fixed byte offsets on every target, each a
/// word-aligned 8-byte step whatever a pointer's width.
pub mod rt {
    /// A sequence header `{ len, cap, data }`.
    pub const SEQ_LEN: i32 = 0;
    pub const SEQ_DATA: i32 = 16;
    /// A string backing `{ rc, length, bytes… }`: the bytes after a 16-byte header.
    pub const STRING_HEADER: i64 = 16;
    /// A closure object `{ count, code, release, captures… }`.
    pub const CLOSURE_CODE: i32 = 8;
    pub const CLOSURE_CAPTURES: u32 = 24;
    /// A task block `{ thread, message, length, arguments… }`.
    pub const TASK_MESSAGE: i32 = 8;
    pub const TASK_LENGTH: i32 = 16;
    pub const TASK_ARGUMENTS: u32 = 24;
    /// What a scope's join writes: `{ message, length }`.
    pub const JOIN_MESSAGE: i32 = 0;
    pub const JOIN_LENGTH: i32 = 8;
}

/// A helper a unit generates on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Helper {
    /// Take a reference to what the value at a pointer owns.
    Retain(Ty),
    /// Give back what the value at a pointer owns.
    Release(Ty),
    /// Whether the values at two pointers are equal.
    Equal(Ty),
    /// Release one element of a `Vec`, or a generic channel's queued slot, for the runtime's
    /// walk.
    ReleaseElement(Ty),
    /// Release what the closure object of this function value's code carries (N50).
    ClosureRelease(mir::FuncId),
}

/// A spawn site's trampoline, defined after the functions.
pub(crate) struct Trampoline {
    pub sym: SymId,
    pub callee: mir::FuncId,
    /// The arguments' types, in the block's order, with their offsets.
    pub args: Vec<(Ty, u32)>,
}

/// Whether a value of `t` is held in memory rather than in a scalar.
#[must_use]
pub const fn in_memory(t: Ty) -> bool {
    matches!(t, Ty::Str | Ty::Record(_) | Ty::Enum(_))
}

/// The class a scalar value of `t` is held in.
#[must_use]
pub const fn class(t: Ty) -> Class {
    match t {
        Ty::Int => Class::I64,
        Ty::Bool => Class::Bit,
        _ => Class::Ptr,
    }
}

/// A runtime word's class.
const fn word(w: Word) -> Class {
    match w {
        Word::I64 => Class::I64,
        Word::I32 => Class::I32,
        Word::Ptr => Class::Ptr,
        Word::Bit => Class::Bit,
    }
}

/// The unit being lowered.
pub(crate) struct Lw<'a> {
    pub native: &'a Native,
    pub unit: &'a Unit,
    pub names: &'a SourceNames<'a>,
    pub lay: ByteLayouts,
    pub data: DataLayout,
    pub debug: bool,
    pub soa: bool,
    symbols: Vec<Symbol>,
    by_name: BTreeMap<String, SymId>,
    pub funcs: BTreeMap<mir::FuncId, SymId>,
    pub functions: Vec<Function>,
    datas: Vec<Data>,
    blobs: BTreeMap<Vec<u8>, SymId>,
    strvals: BTreeMap<Vec<u8>, SymId>,
    messages: BTreeMap<String, (SymId, usize)>,
    pub helpers: BTreeMap<Helper, SymId>,
    pub pending: Vec<Helper>,
    pub trampolines: Vec<Trampoline>,
    pub needs: Needs,
    pub errors: Vec<Diagnostic>,
    anonymous: u32,
}

/// One unit as LIR, on a target's data layout: every function it defines, and every helper,
/// trampoline, closure release and C entry they need. `debug` adds positions and the variables
/// a debugger shows; `soa` lays out every eligible `Vec[R]` field by field (N68).
///
/// # Errors
///
/// A compiler defect, as `N0900`: MIR the validator accepted that this lowering cannot place.
pub fn lower_unit(
    native: &Native,
    unit: &Unit,
    names: &SourceNames<'_>,
    data: DataLayout,
    debug: bool,
    soa: bool,
) -> Result<Module, Vec<Diagnostic>> {
    let lay = ByteLayouts::of(unit, data);
    if let Err(e) = lay.verify() {
        return Err(vec![Diagnostic::error(
            codes::COMPILER_DEFECT,
            Span::synthetic(),
            format!(
                "the layout tables of unit `{}` do not hold: {e}",
                unit.source
            ),
        )]);
    }
    let mut lw = Lw {
        native,
        unit,
        names,
        lay,
        data,
        debug,
        soa,
        symbols: Vec::new(),
        by_name: BTreeMap::new(),
        funcs: BTreeMap::new(),
        functions: Vec::new(),
        datas: Vec::new(),
        blobs: BTreeMap::new(),
        strvals: BTreeMap::new(),
        messages: BTreeMap::new(),
        helpers: BTreeMap::new(),
        pending: Vec::new(),
        trampolines: Vec::new(),
        needs: Needs::default(),
        errors: Vec::new(),
        anonymous: 0,
    };
    lw.declare_functions();
    for id in unit.functions.clone() {
        let (Some(f), Some(sig)) = (native.program.function(id), native.sigs.get(id.index()))
        else {
            continue;
        };
        // A foreign function is never in `unit.functions`: the unit's plan decides that, once
        // (`lower.rs`), and nothing here decides it again (N108).
        let Some(sym) = lw.funcs.get(&id).copied() else {
            continue;
        };
        let lowered = super::func::lower(&mut lw, f, sig, sym);
        lw.functions.push(lowered);
        // An export's C entry, beside the function it calls (N55).
        if let Some(symbol) = &sig.export {
            let c = lw.c_sig(&sig.params, sig.ret);
            let entry = lw.symbol(symbol.as_str(), Linkage::Export, SymKind::Func { sig: c });
            let lowered = super::helpers::export(&mut lw, entry, id, &sig.params, sig.ret);
            lw.functions.push(lowered);
        }
    }
    // Helpers can ask for helpers, and trampolines for helpers: a worklist, each defined once.
    let mut done = 0;
    let mut tdone = 0;
    loop {
        if let Some(h) = lw.pending.get(done).copied() {
            done += 1;
            if let Some(sym) = lw.helpers.get(&h).copied() {
                let lowered = super::helpers::define(&mut lw, sym, h);
                lw.functions.push(lowered);
            }
            continue;
        }
        if let Some((sym, callee, args)) = lw
            .trampolines
            .get(tdone)
            .map(|t| (t.sym, t.callee, t.args.clone()))
        {
            tdone += 1;
            let lowered = super::helpers::trampoline(&mut lw, sym, callee, &args);
            lw.functions.push(lowered);
            continue;
        }
        break;
    }
    if !lw.errors.is_empty() {
        return Err(lw.errors);
    }
    Ok(Module {
        source: unit.source.clone(),
        pointer: data.pointer,
        symbols: lw.symbols,
        functions: lw.functions,
        data: lw.datas,
        needs: lw.needs,
    })
}

impl<'a> Lw<'a> {
    pub(crate) fn defect(&mut self, at: Span, what: impl Into<String>) {
        self.errors.push(
            Diagnostic::error(
                codes::COMPILER_DEFECT,
                at,
                format!(
                    "LIR could not be lowered from validated MIR: {}",
                    what.into()
                ),
            )
            .help("this is a bug in the compiler, not in the program"),
        );
    }

    /// A symbol, interned by name: the first declaration of a name decides it.
    pub(crate) fn symbol(&mut self, name: &str, linkage: Linkage, kind: SymKind) -> SymId {
        if let Some(s) = self.by_name.get(name) {
            return *s;
        }
        let id = SymId(u32::try_from(self.symbols.len()).unwrap_or(u32::MAX));
        self.symbols.push(Symbol {
            name: name.to_owned(),
            linkage,
            kind,
        });
        self.by_name.insert(name.to_owned(), id);
        id
    }

    pub(crate) fn symbol_kind(&self, s: SymId) -> Option<&SymKind> {
        self.symbols.get(s.index()).map(|x| &x.kind)
    }

    /// The internal calling convention (§7.43): an in-memory value by pointer, an in-memory
    /// result through a pointer passed first.
    pub(crate) fn nazm_sig(params: &[Ty], ret: Ty) -> Sig {
        let mut p = Vec::with_capacity(params.len() + 1);
        if in_memory(ret) {
            p.push(Class::Ptr);
        }
        for t in params {
            p.push(if in_memory(*t) { Class::Ptr } else { class(*t) });
        }
        Sig {
            params: p,
            ret: (!in_memory(ret)).then(|| class(ret)),
            conv: Conv::Nazm,
        }
    }

    /// A foreign function's C signature (N42, N55, N85): `Int` an `int64_t`, `Bool` C's
    /// `bool`, a `Str` a borrowed `const char *` and a C-layout record a `const struct *`,
    /// both by pointer; a handle a pointer.
    pub(crate) fn c_sig(&self, params: &[Ty], ret: Ty) -> Sig {
        let c = |t: Ty| match t {
            Ty::Bool => Class::Bit,
            Ty::Int => Class::I64,
            _ => Class::Ptr,
        };
        Sig {
            params: params.iter().map(|t| c(*t)).collect(),
            ret: Some(c(ret)),
            conv: Conv::C,
        }
    }

    /// Declare every Nazm function this unit defines or calls, and every foreign one.
    fn declare_functions(&mut self) {
        let native = self.native;
        let mut wanted: Vec<mir::FuncId> = self.unit.functions.clone();
        for id in &self.unit.functions {
            if let Some(f) = native.program.function(*id) {
                for b in &f.blocks {
                    for s in &b.stmts {
                        match &s.kind {
                            mir::StatementKind::Assign {
                                value: mir::Rvalue::Call { callee, .. },
                                ..
                            }
                            | mir::StatementKind::Spawn { callee, .. }
                            | mir::StatementKind::Assign {
                                value: mir::Rvalue::Closure { func: callee, .. },
                                ..
                            } => wanted.push(*callee),
                            _ => {}
                        }
                    }
                }
            }
        }
        wanted.sort();
        wanted.dedup();
        for id in wanted {
            let Some(sig) = native.sigs.get(id.index()) else {
                self.defect(
                    Span::synthetic(),
                    format!("no signature for MIR function {}", id.0),
                );
                continue;
            };
            let own = self.unit.functions.contains(&id) && !sig.foreign;
            let linkage = if !own {
                Linkage::Import
            } else if sig.linkage == crate::Linkage::Internal {
                Linkage::Local
            } else {
                Linkage::Hidden
            };
            let s = if sig.foreign {
                self.c_sig(&sig.params, sig.ret)
            } else {
                Self::nazm_sig(&sig.params, sig.ret)
            };
            let sym = self.symbol(sig.symbol.as_str(), linkage, SymKind::Func { sig: s });
            self.funcs.insert(id, sym);
        }
    }

    /// A runtime entry point, declared from the registry on first use.
    pub(crate) fn runtime(&mut self, name: &'static str, at: Span) -> SymId {
        let Some(entry) = runtime::import(name) else {
            self.defect(at, format!("`{name}` is not in the runtime registry"));
            return SymId(0);
        };
        match entry.part {
            Part::Strings => self.needs.strings = true,
            Part::Sequences => self.needs.sequences = true,
            Part::Io => self.needs.io = true,
            Part::Join => self.needs.join = true,
            Part::Tasks => self.needs.tasks = true,
            Part::Channels => self.needs.channels = true,
            Part::Closures => self.needs.closures = true,
            Part::Foreign => self.needs.foreign = true,
            Part::Base | Part::Libc => {}
        }
        // The word-level entries this lowering calls are the second compiler's shims (N41).
        self.needs.shims = true;
        if name == "nz.stack_check" {
            self.needs.stack = true;
        }
        // Defined beside the string runtime, whose backings it releases.
        if name == "nz.strs_release" {
            self.needs.strings = true;
        }
        let sig = Sig {
            params: entry.params.iter().map(|w| word(*w)).collect(),
            ret: entry.ret.map(word),
            conv: Conv::Nazm,
        };
        self.symbol(name, Linkage::Import, SymKind::Func { sig })
    }

    /// A runtime data symbol.
    pub(crate) fn runtime_data(&mut self, name: &'static str, class: Class) -> SymId {
        if name == "nz.argc" || name == "nz.argv" {
            self.needs.io = true;
        }
        self.symbol(name, Linkage::Import, SymKind::Data { class: Some(class) })
    }

    fn anonymous(&mut self, what: &str) -> String {
        let n = self.anonymous;
        self.anonymous += 1;
        format!("nz.l.{what}.{n}")
    }

    fn define_data(
        &mut self,
        name: String,
        bytes: Vec<u8>,
        align: u32,
        relocs: Vec<(u32, SymId)>,
    ) -> SymId {
        let sym = self.symbol(&name, Linkage::Local, SymKind::Data { class: None });
        self.datas.push(Data {
            sym,
            bytes,
            align,
            relocs,
        });
        sym
    }

    /// A literal's bytes, read-only. An empty literal still gets a byte, so its pointer is
    /// valid to hand to `memcmp`.
    pub(crate) fn blob(&mut self, bytes: &[u8]) -> SymId {
        if let Some(d) = self.blobs.get(bytes) {
            return *d;
        }
        let name = self.anonymous("s");
        let stored = if bytes.is_empty() {
            vec![0]
        } else {
            bytes.to_vec()
        };
        let id = self.define_data(name, stored, 1, Vec::new());
        self.blobs.insert(bytes.to_vec(), id);
        id
    }

    /// A `Str` literal's whole value — its bytes' address, its length, a null owner, since its
    /// storage is immortal — as read-only data the literal is copied out of.
    pub(crate) fn str_value(&mut self, bytes: &[u8]) -> SymId {
        if let Some(d) = self.strvals.get(bytes) {
            return *d;
        }
        let blob = self.blob(bytes);
        let name = self.anonymous("v");
        let lay = self.data.str();
        let [p, len, _] = self.data.str_fields();
        let mut contents = vec![0u8; lay.size as usize];
        let n = u64::try_from(bytes.len()).unwrap_or(0).to_le_bytes();
        if let Some(slot) = contents.get_mut(len as usize..len as usize + 8) {
            slot.copy_from_slice(&n);
        }
        let id = self.define_data(name, contents, lay.align, vec![(p, blob)]);
        self.strvals.insert(bytes.to_vec(), id);
        id
    }

    /// A failure's full text — `nazm: error[CODE]: message at file:line:col` — the interpreter's
    /// wording, as data, and its length.
    pub(crate) fn message(&mut self, code: &str, message: &str, at: Span) -> (SymId, usize) {
        let pos = self.names.line_col(at);
        let text = format!(
            "nazm: error[{code}]: {message} at {}:{}:{}\n",
            self.names.name(at.file),
            pos.line,
            pos.column
        );
        if let Some(m) = self.messages.get(&text) {
            return *m;
        }
        let name = self.anonymous("m");
        let id = self.define_data(name, text.as_bytes().to_vec(), 1, Vec::new());
        let entry = (id, text.len());
        self.messages.insert(text, entry);
        entry
    }

    /// A generated helper, declared on first use and defined after the functions.
    pub(crate) fn helper(&mut self, h: Helper) -> SymId {
        if let Some(f) = self.helpers.get(&h) {
            return *f;
        }
        let (what, name, params, ret) = match h {
            Helper::Retain(t) => ("ret", self.type_name(t), 1, None),
            Helper::Release(t) => ("rel", self.type_name(t), 1, None),
            Helper::Equal(t) => ("eq", self.type_name(t), 2, Some(Class::Bit)),
            Helper::ReleaseElement(t) => ("vel", self.type_name(t), 1, None),
            Helper::ClosureRelease(f) => ("clo", format!("{}", f.0), 1, None),
        };
        let sig = Sig {
            params: vec![Class::Ptr; params],
            ret,
            conv: Conv::Nazm,
        };
        let sym = self.symbol(
            &format!("nz.h.{what}.{name}"),
            Linkage::Local,
            SymKind::Func { sig },
        );
        self.helpers.insert(h, sym);
        self.pending.push(h);
        sym
    }

    /// A type's name for a helper symbol: the durable native name `nazm-lir` gave it.
    pub(crate) fn type_name(&self, t: Ty) -> String {
        match t {
            Ty::Record(RecordTy(id)) => self
                .unit
                .records
                .iter()
                .find(|r| r.id.0 == id)
                .map_or_else(|| format!("r{id}"), |r| r.name.as_str().to_owned()),
            Ty::Enum(EnumTy(id)) => self
                .unit
                .enums
                .iter()
                .find(|e| e.id.0 == id)
                .map_or_else(|| format!("e{id}"), |e| e.name.as_str().to_owned()),
            Ty::Vec(v) => self
                .unit
                .vecs
                .iter()
                .find(|l| l.id == v)
                .map_or_else(|| format!("v{}", v.0), |l| format!("vec.{}", l.name)),
            // One name per generic channel type, as a `Vec`'s is per element (N54).
            Ty::ChanOf(c) => format!("chan.{}", c.0),
            other => other.name().to_owned(),
        }
    }

    /// A trampoline for a spawn site, declared now and defined after the functions.
    pub(crate) fn trampoline(&mut self, callee: mir::FuncId, args: Vec<(Ty, u32)>) -> SymId {
        let name = self.anonymous("task");
        let sig = Sig {
            params: vec![Class::Ptr],
            ret: Some(Class::Ptr),
            conv: Conv::C,
        };
        let sym = self.symbol(&name, Linkage::Local, SymKind::Func { sig });
        self.trampolines.push(Trampoline { sym, callee, args });
        sym
    }

    /// The size of a value of `t` in memory.
    pub(crate) fn size(&self, t: Ty) -> u32 {
        self.lay.lay(t).map_or(8, |l| l.size)
    }

    /// The position a debugger should say `at` is.
    pub(crate) fn pos(&self, at: Span) -> Option<Pos> {
        if !self.debug {
            return None;
        }
        self.names
            .position(at)
            .map(|(file, line, column)| Pos { file, line, column })
    }
}

/// An instruction stream under construction: one function's blocks, values, variables and
/// slots.
pub(crate) struct Fb {
    pub blocks: Vec<Block>,
    /// The block instructions are being added to; `None` after a terminator, until a block is
    /// switched to.
    pub current: Option<B>,
    pub values: u32,
    pub vars: Vec<Class>,
    pub slots: Vec<SlotDecl>,
    pub var_names: Vec<String>,
    pub slot_names: Vec<String>,
    pub debug_vars: Vec<DebugVar>,
}

impl Fb {
    pub(crate) fn new() -> Self {
        let mut fb = Self {
            blocks: Vec::new(),
            current: None,
            values: 0,
            vars: Vec::new(),
            slots: Vec::new(),
            var_names: Vec::new(),
            slot_names: Vec::new(),
            debug_vars: Vec::new(),
        };
        let entry = fb.block();
        fb.switch(entry);
        fb
    }

    pub(crate) fn block(&mut self) -> B {
        let b = B(u32::try_from(self.blocks.len()).unwrap_or(u32::MAX));
        self.blocks.push(Block {
            insts: Vec::new(),
            term: Term::Unreachable,
        });
        b
    }

    pub(crate) fn switch(&mut self, b: B) {
        self.current = Some(b);
    }

    pub(crate) fn value(&mut self) -> V {
        let v = V(self.values);
        self.values += 1;
        v
    }

    pub(crate) fn var(&mut self, c: Class) -> Var {
        let v = Var(u32::try_from(self.vars.len()).unwrap_or(u32::MAX));
        self.vars.push(c);
        self.var_names.push(String::new());
        v
    }

    pub(crate) fn slot(&mut self, size: u32, align: u32) -> super::ir::Slot {
        let s = super::ir::Slot(u32::try_from(self.slots.len()).unwrap_or(u32::MAX));
        self.slots.push(SlotDecl {
            size: size.max(1),
            align: align.max(1),
        });
        self.slot_names.push(String::new());
        s
    }

    pub(crate) fn push(&mut self, i: super::ir::Inst) {
        // An instruction after a terminator has nowhere to go: it is unreachable code, and
        // dropped, so every block keeps exactly one terminator.
        if let Some(b) = self.current {
            if let Some(blk) = self.blocks.get_mut(b.index()) {
                blk.insts.push(i);
            }
        }
    }

    pub(crate) fn term(&mut self, t: Term) {
        if let Some(b) = self.current.take() {
            if let Some(blk) = self.blocks.get_mut(b.index()) {
                blk.term = t;
            }
        }
    }

    pub(crate) fn finish(self, sym: SymId, name: String, at: Option<(u32, u32)>) -> Function {
        Function {
            sym,
            name,
            vars: self.vars,
            slots: self.slots,
            var_names: self.var_names,
            slot_names: self.slot_names,
            blocks: self.blocks,
            values: self.values,
            at,
            debug_vars: self.debug_vars,
        }
    }
}
