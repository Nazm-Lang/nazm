//! One unit, as one object module.
//!
//! Declares everything a unit's code can reach — its own functions and the ones it calls in
//! other units, the runtime's entry points (only through [`nazm_lir::runtime`]), the data its
//! literals and failure messages live in — then defines each MIR function, and last the helpers
//! and trampolines those definitions asked for.

use std::collections::BTreeMap;

use cranelift_codegen::ir::{types, AbiParam, Signature, Type};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use nazm_diag::{codes, Diagnostic};
use nazm_lir::runtime::{self, Word};
use nazm_lir::{EnumTy, Native, Needs, RecordTy, Ty, Unit};
use nazm_mir as mir;
use nazm_span::Span;

use crate::layout::{in_memory, Layouts};

/// A helper this unit generates on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Helper {
    /// Take a reference to what the value at a pointer owns.
    Retain(Ty),
    /// Give back what the value at a pointer owns.
    Release(Ty),
    /// Whether the values at two pointers are equal.
    Equal(Ty),
    /// Release one element of a `Vec`, for the runtime's walk.
    ReleaseElement(Ty),
}

/// A spawn site's trampoline, to define after the functions.
pub(crate) struct Trampoline {
    pub id: FuncId,
    pub callee: mir::FuncId,
    /// The arguments' types, in the block's order, with their offsets.
    pub args: Vec<(Ty, u32)>,
}

pub(crate) struct Gen<'a> {
    pub module: ObjectModule,
    pub native: &'a Native,
    pub unit: &'a Unit,
    pub names: &'a nazm_lir::SourceNames<'a>,
    pub lay: Layouts,
    pub ptr: Type,
    pub funcs: BTreeMap<mir::FuncId, FuncId>,
    imports: BTreeMap<&'static str, FuncId>,
    data_imports: BTreeMap<&'static str, DataId>,
    blobs: BTreeMap<Vec<u8>, DataId>,
    strvals: BTreeMap<Vec<u8>, DataId>,
    messages: BTreeMap<String, (DataId, usize)>,
    pub helpers: BTreeMap<Helper, FuncId>,
    pub pending: Vec<Helper>,
    pub trampolines: Vec<Trampoline>,
    pub needs: Needs,
    pub errors: Vec<Diagnostic>,
    anonymous: u32,
}

impl<'a> Gen<'a> {
    pub(crate) fn new(
        isa: OwnedTargetIsa,
        native: &'a Native,
        unit: &'a Unit,
        names: &'a nazm_lir::SourceNames<'a>,
        name: &str,
    ) -> Result<Self, String> {
        let builder = ObjectBuilder::new(isa, name, cranelift_module::default_libcall_names())
            .map_err(|e| e.to_string())?;
        let module = ObjectModule::new(builder);
        let ptr = module.target_config().pointer_type();
        Ok(Self {
            module,
            native,
            unit,
            names,
            lay: Layouts::of(unit),
            ptr,
            funcs: BTreeMap::new(),
            imports: BTreeMap::new(),
            data_imports: BTreeMap::new(),
            blobs: BTreeMap::new(),
            strvals: BTreeMap::new(),
            messages: BTreeMap::new(),
            helpers: BTreeMap::new(),
            pending: Vec::new(),
            trampolines: Vec::new(),
            needs: Needs {
                shims: true,
                ..Needs::default()
            },
            errors: Vec::new(),
            anonymous: 0,
        })
    }

    pub(crate) fn defect(&mut self, at: Span, what: impl Into<String>) {
        self.errors.push(
            Diagnostic::error(
                codes::COMPILER_DEFECT,
                at,
                format!(
                    "the Cranelift backend could not compile validated MIR: {}",
                    what.into()
                ),
            )
            .help("this is a bug in the compiler, not in the program"),
        );
    }

    /// The machine type a value of `t` is held in when it is not in memory.
    pub(crate) const fn clif(t: Ty) -> Type {
        match t {
            Ty::Bool => types::I8,
            _ => types::I64,
        }
    }

    /// The internal calling convention (§7.43): in-memory values by pointer, an in-memory
    /// result through a pointer passed first.
    pub(crate) fn signature(&self, params: &[Ty], ret: Ty) -> Signature {
        let mut s = self.module.make_signature();
        if in_memory(ret) {
            s.params.push(AbiParam::new(self.ptr));
        } else {
            s.returns.push(AbiParam::new(Self::clif(ret)));
        }
        for p in params {
            s.params.push(AbiParam::new(if in_memory(*p) {
                self.ptr
            } else {
                Self::clif(*p)
            }));
        }
        s
    }

    /// A foreign function's C signature (N42): only `Int` (`int64_t`) and `Bool` (C's `bool`,
    /// a zero-extended byte) cross, both by value, so no pointer and no sret appear.
    pub(crate) fn c_signature(&self, params: &[Ty], ret: Ty) -> Signature {
        let c = |t: Ty| {
            if t == Ty::Bool {
                AbiParam::new(types::I8).uext()
            } else {
                AbiParam::new(types::I64)
            }
        };
        let mut s = self.module.make_signature();
        s.returns.push(c(ret));
        s.params.extend(params.iter().map(|p| c(*p)));
        s
    }

    /// Declare every function this unit defines or calls.
    pub(crate) fn declare_functions(&mut self) -> Result<(), String> {
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
                            | mir::StatementKind::Spawn { callee, .. } => wanted.push(*callee),
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
                return Err(format!("no signature for MIR function {}", id.0));
            };
            let own = self.unit.functions.contains(&id);
            let linkage = if !own {
                Linkage::Import
            } else if sig.linkage == nazm_lir::Linkage::Internal {
                Linkage::Local
            } else {
                Linkage::Hidden
            };
            let signature = if sig.foreign {
                self.c_signature(&sig.params, sig.ret)
            } else {
                self.signature(&sig.params, sig.ret)
            };
            let f = self
                .module
                .declare_function(sig.symbol.as_str(), linkage, &signature)
                .map_err(|e| e.to_string())?;
            self.funcs.insert(id, f);
        }
        Ok(())
    }

    /// A runtime entry point, declared from the registry on first use.
    pub(crate) fn import(&mut self, symbol: &'static str) -> FuncId {
        if let Some(f) = self.imports.get(symbol) {
            return *f;
        }
        let Some(entry) = runtime::import(symbol) else {
            self.defect(
                Span::synthetic(),
                format!("`{symbol}` is not in the runtime registry"),
            );
            return FuncId::from_u32(0);
        };
        match entry.part {
            runtime::Part::Strings => self.needs.strings = true,
            runtime::Part::Sequences => self.needs.sequences = true,
            runtime::Part::Io => self.needs.io = true,
            runtime::Part::Join => self.needs.join = true,
            runtime::Part::Tasks => self.needs.tasks = true,
            runtime::Part::Channels => self.needs.channels = true,
            runtime::Part::Base | runtime::Part::Libc => {}
        }
        let word = |w: Word| match w {
            Word::I64 | Word::Ptr => types::I64,
            Word::I32 => types::I32,
            Word::Bit => types::I8,
        };
        let mut s = self.module.make_signature();
        for p in entry.params {
            s.params.push(AbiParam::new(word(*p)));
        }
        if let Some(r) = entry.ret {
            s.returns.push(AbiParam::new(word(r)));
        }
        let f = match self.module.declare_function(symbol, Linkage::Import, &s) {
            Ok(f) => f,
            Err(e) => {
                self.defect(Span::synthetic(), e.to_string());
                FuncId::from_u32(0)
            }
        };
        self.imports.insert(symbol, f);
        f
    }

    /// A runtime data symbol, declared on first use.
    pub(crate) fn data_import(&mut self, symbol: &'static str) -> DataId {
        if let Some(d) = self.data_imports.get(symbol) {
            return *d;
        }
        if symbol == "nz.argc" || symbol == "nz.argv" {
            self.needs.io = true;
        }
        let d = match self
            .module
            .declare_data(symbol, Linkage::Import, true, false)
        {
            Ok(d) => d,
            Err(e) => {
                self.defect(Span::synthetic(), e.to_string());
                DataId::from_u32(0)
            }
        };
        self.data_imports.insert(symbol, d);
        d
    }

    fn anonymous(&mut self, what: &str) -> String {
        let n = self.anonymous;
        self.anonymous += 1;
        format!("nz.c.{what}.{n}")
    }

    /// The bytes of a literal, as read-only data. An empty literal still gets a byte, so its
    /// pointer is valid to hand to `memcmp`.
    pub(crate) fn blob(&mut self, bytes: &[u8]) -> DataId {
        if let Some(d) = self.blobs.get(bytes) {
            return *d;
        }
        let name = self.anonymous("s");
        let id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .unwrap_or_else(|_| DataId::from_u32(0));
        let mut desc = DataDescription::new();
        let stored: Vec<u8> = if bytes.is_empty() {
            vec![0]
        } else {
            bytes.to_vec()
        };
        desc.define(stored.into_boxed_slice());
        if let Err(e) = self.module.define_data(id, &desc) {
            self.defect(Span::synthetic(), e.to_string());
        }
        self.blobs.insert(bytes.to_vec(), id);
        id
    }

    /// A `Str` literal's whole value — pointer, length, a null owner, since its storage is
    /// immortal — as read-only data the literal is copied out of.
    pub(crate) fn str_value(&mut self, bytes: &[u8]) -> DataId {
        if let Some(d) = self.strvals.get(bytes) {
            return *d;
        }
        let blob = self.blob(bytes);
        let name = self.anonymous("v");
        let id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .unwrap_or_else(|_| DataId::from_u32(0));
        let mut desc = DataDescription::new();
        let mut contents = vec![0u8; 24];
        let len = u64::try_from(bytes.len()).unwrap_or(0).to_le_bytes();
        if let Some(slot) = contents.get_mut(8..16) {
            slot.copy_from_slice(&len);
        }
        desc.define(contents.into_boxed_slice());
        desc.set_align(8);
        let gv = self.module.declare_data_in_data(blob, &mut desc);
        desc.write_data_addr(0, gv, 0);
        if let Err(e) = self.module.define_data(id, &desc) {
            self.defect(Span::synthetic(), e.to_string());
        }
        self.strvals.insert(bytes.to_vec(), id);
        id
    }

    /// A failure's full text — `nazm: error[CODE]: message at file:line:col` — the interpreter's
    /// wording, as data, and its length.
    pub(crate) fn message(&mut self, code: &str, message: &str, at: Span) -> (DataId, usize) {
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
        let blob = self.blob(text.as_bytes());
        let entry = (blob, text.len());
        self.messages.insert(text, entry);
        entry
    }

    /// A generated helper, declared on first use and defined after the functions.
    pub(crate) fn helper(&mut self, h: Helper) -> FuncId {
        if let Some(f) = self.helpers.get(&h) {
            return *f;
        }
        let (what, name, params, ret) = match h {
            Helper::Retain(t) => ("ret", self.type_name(t), 1, None),
            Helper::Release(t) => ("rel", self.type_name(t), 1, None),
            Helper::Equal(t) => ("eq", self.type_name(t), 2, Some(types::I8)),
            Helper::ReleaseElement(t) => ("vel", self.type_name(t), 1, None),
        };
        let mut s = self.module.make_signature();
        for _ in 0..params {
            s.params.push(AbiParam::new(self.ptr));
        }
        if let Some(r) = ret {
            s.returns.push(AbiParam::new(r));
        }
        let symbol = format!("nz.c.{what}.{name}");
        let f = match self.module.declare_function(&symbol, Linkage::Local, &s) {
            Ok(f) => f,
            Err(e) => {
                self.defect(Span::synthetic(), e.to_string());
                FuncId::from_u32(0)
            }
        };
        self.helpers.insert(h, f);
        self.pending.push(h);
        f
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
            other => other.name().to_owned(),
        }
    }

    /// Define every function of the unit, then every helper and trampoline they asked for.
    pub(crate) fn run(mut self, verify: bool) -> Result<(Vec<u8>, Needs), Vec<Diagnostic>> {
        if let Err(e) = self.declare_functions() {
            self.defect(Span::synthetic(), e);
        }
        let mut fctx = FunctionBuilderContext::new();
        for id in self.unit.functions.clone() {
            let Some(f) = self.native.program.function(id) else {
                continue;
            };
            let Some(sig) = self.native.sigs.get(id.index()) else {
                continue;
            };
            let Some(fid) = self.funcs.get(&id).copied() else {
                continue;
            };
            let mut ctx = self.module.make_context();
            crate::func::translate(&mut self, &mut ctx, &mut fctx, f, sig);
            self.define(fid, &mut ctx, verify, &f.name);
        }
        // Helpers can ask for helpers (a record's release releases its fields'), so this is a
        // worklist; each is defined once.
        let mut done = 0;
        while done < self.pending.len() {
            let Some(h) = self.pending.get(done).copied() else {
                break;
            };
            done += 1;
            let Some(fid) = self.helpers.get(&h).copied() else {
                continue;
            };
            let mut ctx = self.module.make_context();
            crate::helpers::define(&mut self, &mut ctx, &mut fctx, h);
            self.define(fid, &mut ctx, verify, "a helper");
        }
        let trampolines = std::mem::take(&mut self.trampolines);
        for t in &trampolines {
            let mut ctx = self.module.make_context();
            crate::helpers::trampoline(&mut self, &mut ctx, &mut fctx, t);
            self.define(t.id, &mut ctx, verify, "a task trampoline");
            // A trampoline may have asked for release helpers.
            while done < self.pending.len() {
                let Some(h) = self.pending.get(done).copied() else {
                    break;
                };
                done += 1;
                let Some(fid) = self.helpers.get(&h).copied() else {
                    continue;
                };
                let mut ctx = self.module.make_context();
                crate::helpers::define(&mut self, &mut ctx, &mut fctx, h);
                self.define(fid, &mut ctx, verify, "a helper");
            }
        }
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        let needs = self.needs;
        match self.module.finish().emit() {
            Ok(bytes) => Ok((bytes, needs)),
            Err(e) => Err(vec![Diagnostic::error(
                codes::COMPILER_DEFECT,
                Span::synthetic(),
                format!("the Cranelift backend could not write an object: {e}"),
            )]),
        }
    }

    fn define(&mut self, id: FuncId, ctx: &mut Context, verify: bool, what: &str) {
        if verify {
            if let Err(e) = cranelift_codegen::verify_function(&ctx.func, self.module.isa()) {
                self.defect(
                    Span::synthetic(),
                    format!("Cranelift's verifier rejected `{what}`: {e}"),
                );
                return;
            }
        }
        if let Err(e) = self.module.define_function(id, ctx) {
            self.defect(Span::synthetic(), format!("`{what}`: {e:?}"));
        }
    }

    /// A trampoline for a spawn site, declared now and defined after the functions.
    pub(crate) fn trampoline(&mut self, callee: mir::FuncId, args: Vec<(Ty, u32)>) -> FuncId {
        let name = self.anonymous("task");
        let mut s = self.module.make_signature();
        s.params.push(AbiParam::new(self.ptr));
        s.returns.push(AbiParam::new(self.ptr));
        let id = match self.module.declare_function(&name, Linkage::Local, &s) {
            Ok(f) => f,
            Err(e) => {
                self.defect(Span::synthetic(), e.to_string());
                FuncId::from_u32(0)
            }
        };
        self.trampolines.push(Trampoline { id, callee, args });
        id
    }
}
