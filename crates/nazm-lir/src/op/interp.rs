//! An interpreter of LIR: the oracle the native backends are held to (N105, §7.106).
//!
//! It executes a program's modules — every unit, linked by symbol name — against a model of the
//! runtime's services written here, and ends as the platform entry does: `main`'s result printed
//! and status 0, or the first failure's text and status 2. Memory is one flat arena of bytes, so
//! every load, store and offset LIR states is executed as stated.
//!
//! # The sequential subset
//!
//! Tasks, channels, the clock, device registers and foreign calls are refused by name
//! ([`Stop::Refused`]): the oracle covers what a single thread does with the language's values.
//! The runtime model is not the runtime: it is a second statement of the same services' meaning,
//! so a program on which it and a native build disagree has found a defect in one of them.

#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::integer_division,
    reason = "a model of the machine: a value is truncated as the machine truncates it, values and \
              arena bytes are indexed by what the validator has already checked, and a panic is \
              caught by `nazm lir --run` and reported as a defect, never as an answer"
)]

use std::collections::BTreeMap;

use super::ir::{BinOp, Class, Cmp, ConvOp, Inst, Linkage, Module, OvfOp, SymKind, Term, UnOp};
use super::lower::rt;

/// How a run ended, as the platform entry would have ended it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Why a run did not end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Outside the oracle's subset.
    Refused(String),
    /// The module broke a rule the validator should have caught, or the program did what no
    /// native build could survive (a null dereference).
    Defect(String),
    /// More instructions than the budget allowed.
    Budget,
}

const BASE: u64 = 4096;
const FN_BASE: u64 = 0x7000_0000_0000;
const DEPTH: usize = 20_000;

/// A function or runtime entry a call can reach.
#[derive(Debug, Clone, Copy)]
enum Target {
    Code { module: usize, function: usize },
    Runtime(&'static str),
    Data(u64),
}

struct Machine<'m> {
    modules: &'m [Module],
    mem: Vec<u8>,
    /// Each module's symbols, resolved.
    resolved: Vec<Vec<Option<Target>>>,
    /// Function addresses, by index into `codes`.
    codes: Vec<(usize, usize)>,
    /// Reference counts of the runtime's objects, by address.
    counts: BTreeMap<u64, i64>,
    sizes: BTreeMap<u64, u64>,
    failed: Option<Vec<u8>>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exited: Option<i32>,
    budget: u64,
    depth: usize,
    args: Vec<Vec<u8>>,
    argc: u64,
    argv: u64,
    /// Each function's values' classes, computed on its first call.
    classes: BTreeMap<(usize, usize), std::rc::Rc<Vec<Class>>>,
    /// The sequences queued by the release in progress, as the runtime's queue holds them
    /// (R1a): handle, element release, stride.
    releasing: Vec<(u64, u64, u64)>,
}

/// Run a program: its modules, its entry's symbol, how many capability roots `main` takes, its
/// arguments, and a budget of instructions.
///
/// # Errors
///
/// [`Stop`]: outside the subset, a defect, or the budget spent.
pub fn run(
    modules: &[Module],
    entry: &str,
    roots: usize,
    args: &[String],
    budget: u64,
) -> Result<Outcome, Stop> {
    let mut m = Machine {
        modules,
        mem: vec![0; BASE as usize],
        resolved: Vec::new(),
        codes: Vec::new(),
        counts: BTreeMap::new(),
        releasing: Vec::new(),
        sizes: BTreeMap::new(),
        failed: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        exited: None,
        budget,
        depth: 0,
        args: args.iter().map(|a| a.as_bytes().to_vec()).collect(),
        argc: 0,
        argv: 0,
        classes: BTreeMap::new(),
    };
    m.link()?;
    m.arguments();
    let (module, function) = m
        .find_code(entry)
        .ok_or_else(|| Stop::Defect(format!("no definition of the entry `{entry}`")))?;
    let roots = vec![0u128; roots];
    let result = m.call(module, function, &roots)?;
    if let Some(code) = m.exited {
        return Ok(m.outcome(code));
    }
    if let Some(msg) = m.failed.take() {
        m.stderr.extend_from_slice(&msg);
        return Ok(m.outcome(2));
    }
    let r = result.unwrap_or(0) as i64;
    // `main`'s result is the exit status unless the result report is asked for (R1, 5b.3), as
    // the compiled program's entry does it.
    if std::env::var_os(crate::RESULT_VARIABLE).is_some() {
        m.stdout.extend_from_slice(format!("{r}\n").as_bytes());
        return Ok(m.outcome(0));
    }
    match u8::try_from(r) {
        Ok(code) => Ok(m.outcome(i32::from(code))),
        Err(_) => {
            m.stderr
                .extend_from_slice(format!("{}\n", crate::RESULT_STATUS).as_bytes());
            Ok(m.outcome(2))
        }
    }
}

impl Machine<'_> {
    fn outcome(&mut self, code: i32) -> Outcome {
        Outcome {
            code,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
        }
    }

    fn find_code(&self, name: &str) -> Option<(usize, usize)> {
        for (mi, m) in self.modules.iter().enumerate() {
            for (fi, f) in m.functions.iter().enumerate() {
                let s = m.symbol(f.sym)?;
                if s.name == name && s.linkage != Linkage::Local {
                    return Some((mi, fi));
                }
            }
        }
        None
    }

    // --- memory --------------------------------------------------------------------------

    fn alloc(&mut self, n: u64) -> u64 {
        let at = (self.mem.len() as u64).div_ceil(16) * 16;
        let size = n.max(1);
        self.mem.resize((at + size) as usize, 0);
        self.sizes.insert(at, size);
        at
    }

    fn check(&self, a: u64, n: u64) -> Result<(), Stop> {
        if a < BASE || a.saturating_add(n) > self.mem.len() as u64 {
            return Err(Stop::Defect(format!("an access of {n} bytes at {a:#x}")));
        }
        Ok(())
    }

    fn read(&self, a: u64, n: u64) -> Result<u64, Stop> {
        self.check(a, n)?;
        let mut v = 0u64;
        for i in (0..n).rev() {
            v = (v << 8) | u64::from(self.mem[(a + i) as usize]);
        }
        Ok(v)
    }

    fn write(&mut self, a: u64, n: u64, v: u64) -> Result<(), Stop> {
        self.check(a, n)?;
        for i in 0..n {
            self.mem[(a + i) as usize] = (v >> (8 * i)) as u8;
        }
        Ok(())
    }

    /// A load of up to 16 bytes: an `I128` (R1b, L13b) is two words, low first.
    fn read_wide(&self, a: u64, n: u64) -> Result<u128, Stop> {
        if n <= 8 {
            return Ok(u128::from(self.read(a, n)?));
        }
        let lo = self.read(a, 8)?;
        let hi = self.read(a + 8, n - 8)?;
        Ok(u128::from(lo) | (u128::from(hi) << 64))
    }

    /// A store of up to 16 bytes.
    fn write_wide(&mut self, a: u64, n: u64, v: u128) -> Result<(), Stop> {
        if n <= 8 {
            return self.write(a, n, v as u64);
        }
        self.write(a, 8, v as u64)?;
        self.write(a + 8, n - 8, (v >> 64) as u64)
    }

    fn bytes(&self, a: u64, n: u64) -> Result<Vec<u8>, Stop> {
        if n == 0 {
            return Ok(Vec::new());
        }
        self.check(a, n)?;
        Ok(self.mem[a as usize..(a + n) as usize].to_vec())
    }

    fn put(&mut self, a: u64, b: &[u8]) -> Result<(), Stop> {
        if b.is_empty() {
            return Ok(());
        }
        self.check(a, b.len() as u64)?;
        self.mem[a as usize..a as usize + b.len()].copy_from_slice(b);
        Ok(())
    }

    fn arguments(&mut self) {
        let args = self.args.clone();
        self.argc = args.len() as u64;
        let table = self.alloc(8 * (args.len() as u64 + 1));
        // `argv[0]`, the program's own name, which no program sees.
        let name = self.alloc(1);
        let _ = self.write(table, 8, name);
        for (i, a) in args.iter().enumerate() {
            let p = self.alloc(a.len() as u64 + 1);
            let _ = self.put(p, a);
            let _ = self.write(table + 8 * (i as u64 + 1), 8, p);
        }
        self.argv = table;
    }

    // --- linking -------------------------------------------------------------------------

    fn link(&mut self) -> Result<(), Stop> {
        // Every definition, by name, for the imports of other modules to bind to.
        let mut defined: BTreeMap<String, Target> = BTreeMap::new();
        let mut data_at: Vec<BTreeMap<u32, u64>> = Vec::new();
        for (mi, m) in self.modules.iter().enumerate() {
            let mut here = BTreeMap::new();
            for d in &m.data {
                let at = self.alloc(d.bytes.len() as u64);
                let _ = self.put(at, &d.bytes);
                here.insert(d.sym.0, at);
                // A static (Gate 3) is reached from every unit by its name.
                if let Some(s) = m.symbol(d.sym) {
                    if s.linkage != Linkage::Local {
                        defined.insert(s.name.clone(), Target::Data(at));
                    }
                }
            }
            data_at.push(here);
            for (fi, f) in m.functions.iter().enumerate() {
                self.codes.push((mi, fi));
                if let Some(s) = m.symbol(f.sym) {
                    if s.linkage != Linkage::Local {
                        defined.insert(
                            s.name.clone(),
                            Target::Code {
                                module: mi,
                                function: fi,
                            },
                        );
                    }
                }
            }
        }
        for (mi, m) in self.modules.iter().enumerate() {
            let mut out = Vec::with_capacity(m.symbols.len());
            for (si, s) in m.symbols.iter().enumerate() {
                let local_fn = m
                    .functions
                    .iter()
                    .position(|f| f.sym.index() == si)
                    .map(|fi| Target::Code {
                        module: mi,
                        function: fi,
                    });
                let local_data = data_at[mi].get(&(si as u32)).map(|a| Target::Data(*a));
                let t = match (&s.kind, s.linkage) {
                    (_, Linkage::Import) => match &s.kind {
                        SymKind::Func { .. } => defined
                            .get(&s.name)
                            .copied()
                            .or_else(|| runtime_name(&s.name).map(Target::Runtime)),
                        SymKind::Data { .. } => defined
                            .get(&s.name)
                            .copied()
                            .or_else(|| runtime_name(&s.name).map(Target::Runtime)),
                    },
                    (SymKind::Func { .. }, _) => local_fn,
                    (SymKind::Data { .. }, _) => local_data,
                };
                out.push(t);
            }
            self.resolved.push(out);
        }
        // Relocations: each address written into its data.
        for (mi, m) in self.modules.iter().enumerate() {
            for d in &m.data {
                let Some(base) = data_at[mi].get(&d.sym.0).copied() else {
                    continue;
                };
                for (off, s) in &d.relocs {
                    let a = self.address(mi, s.index())?;
                    self.write(base + u64::from(*off), u64::from(m.pointer), a)?;
                }
            }
        }
        Ok(())
    }

    /// The address of a module's symbol: data's, or a function's code.
    fn address(&mut self, module: usize, sym: usize) -> Result<u64, Stop> {
        match self
            .resolved
            .get(module)
            .and_then(|r| r.get(sym))
            .copied()
            .flatten()
        {
            Some(Target::Data(a)) => Ok(a),
            Some(Target::Code { module, function }) => {
                let i = self
                    .codes
                    .iter()
                    .position(|c| *c == (module, function))
                    .unwrap_or(0);
                Ok(FN_BASE + 16 * i as u64)
            }
            Some(Target::Runtime("nz.argc")) => {
                let a = self.alloc(8);
                self.write(a, 8, self.argc)?;
                Ok(a)
            }
            Some(Target::Runtime("nz.argv")) => {
                let a = self.alloc(8);
                self.write(a, 8, self.argv)?;
                Ok(a)
            }
            Some(Target::Runtime(name)) => Err(Stop::Refused(format!(
                "the address of the runtime's `{name}`"
            ))),
            None => {
                let name = self.modules[module]
                    .symbols
                    .get(sym)
                    .map_or("?", |s| s.name.as_str());
                Err(Stop::Refused(format!(
                    "`{name}`, which no module defines: C's"
                )))
            }
        }
    }

    // --- execution -----------------------------------------------------------------------

    /// Call a function at a code address.
    fn call_address(&mut self, addr: u64, args: &[u128]) -> Result<Option<u128>, Stop> {
        Ok(self.call_address_failing(addr, args)?.0)
    }

    /// Call a function at a code address: its result, and whether it left by its failure exit.
    fn call_address_failing(
        &mut self,
        addr: u64,
        args: &[u128],
    ) -> Result<(Option<u128>, bool), Stop> {
        let i = addr
            .checked_sub(FN_BASE)
            .map(|d| (d / 16) as usize)
            .and_then(|i| self.codes.get(i).copied());
        match i {
            Some((m, f)) => self.call_failing(m, f, args),
            None => Err(Stop::Defect(format!(
                "a call to {addr:#x}, which is no code"
            ))),
        }
    }

    fn call(&mut self, mi: usize, fi: usize, args: &[u128]) -> Result<Option<u128>, Stop> {
        Ok(self.call_failing(mi, fi, args)?.0)
    }

    /// Run a function: its result, and its failure bit (R1, 5b.8) — whether it left by
    /// [`Term::Fail`], which only a function that has recorded a failure may.
    #[allow(clippy::too_many_lines)]
    fn call_failing(
        &mut self,
        mi: usize,
        fi: usize,
        args: &[u128],
    ) -> Result<(Option<u128>, bool), Stop> {
        let modules = self.modules;
        let m = &modules[mi];
        let f = &m.functions[fi];
        self.depth += 1;
        if self.depth > DEPTH {
            return Err(Stop::Refused("a call depth beyond the oracle's".to_owned()));
        }
        let classes = match self.classes.get(&(mi, fi)) {
            Some(c) => c.clone(),
            None => {
                let c = std::rc::Rc::new(value_classes(f, m));
                self.classes.insert((mi, fi), c.clone());
                c
            }
        };
        // A value is 128 bits (R1b, L13b): an `I128` fills it, every other class its low bits.
        let mut vals = vec![0u128; f.values as usize];
        let mut vars = vec![0u128; f.vars.len()];
        let slots: Vec<u64> = f
            .slots
            .iter()
            .map(|s| self.alloc(u64::from(s.size)))
            .collect();
        let width = |c: Class, p: u32| -> u64 {
            match c {
                Class::Bit | Class::I8 => 1,
                Class::I16 => 2,
                Class::I32 | Class::F32 => 4,
                Class::I64 | Class::F64 => 8,
                Class::I128 => 16,
                Class::Ptr => u64::from(p),
            }
        };
        let mut b = 0usize;
        loop {
            let Some(block) = f.blocks.get(b) else {
                return Err(Stop::Defect(format!("no block b{b}")));
            };
            for i in &block.insts {
                if self.budget == 0 {
                    return Err(Stop::Budget);
                }
                self.budget -= 1;
                match i {
                    Inst::Param { dst, index } => {
                        vals[dst.index()] = args.get(*index as usize).copied().unwrap_or(0);
                    }
                    Inst::Const { dst, class, value } => {
                        vals[dst.index()] = norm(*class, *value as u128);
                    }
                    Inst::Addr { dst, sym } => {
                        vals[dst.index()] = u128::from(self.address(mi, sym.index())?);
                    }
                    Inst::SlotAddr { dst, slot } => {
                        vals[dst.index()] =
                            u128::from(slots.get(slot.0 as usize).copied().unwrap_or(0));
                    }
                    Inst::Load {
                        dst,
                        class,
                        addr,
                        offset,
                    } => {
                        let a = (vals[addr.index()] as u64).wrapping_add(*offset as i64 as u64);
                        let v = self.read_wide(a, width(*class, m.pointer))?;
                        vals[dst.index()] = norm(*class, v);
                    }
                    Inst::Store {
                        addr,
                        offset,
                        value,
                    } => {
                        let a = (vals[addr.index()] as u64).wrapping_add(*offset as i64 as u64);
                        let n = width(
                            classes.get(value.index()).copied().unwrap_or(Class::I64),
                            m.pointer,
                        );
                        self.write_wide(a, n, vals[value.index()])?;
                    }
                    Inst::Bin { dst, op, a, b: y } => {
                        let cl = classes.get(a.index()).copied().unwrap_or(Class::I64);
                        vals[dst.index()] = bin_op(*op, cl, vals[a.index()], vals[y.index()])?;
                    }
                    Inst::Ovf {
                        dst,
                        ovf,
                        op,
                        a,
                        b: y,
                    } => {
                        let cl = classes.get(a.index()).copied().unwrap_or(Class::I64);
                        let (r, o) = ovf_op(*op, cl, vals[a.index()], vals[y.index()]);
                        vals[dst.index()] = r;
                        vals[ovf.index()] = u128::from(o);
                    }
                    Inst::Cmp { dst, op, a, b: y } => {
                        let cl = classes.get(a.index()).copied().unwrap_or(Class::I64);
                        let r = cmp_op(*op, cl, vals[a.index()], vals[y.index()]);
                        vals[dst.index()] = u128::from(r);
                    }
                    Inst::Select { dst, cond, a, b: y } => {
                        vals[dst.index()] = if vals[cond.index()] & 1 == 1 {
                            vals[a.index()]
                        } else {
                            vals[y.index()]
                        };
                    }
                    Inst::Ext {
                        dst,
                        to,
                        src,
                        signed: sign,
                    } => {
                        let from = classes.get(src.index()).copied().unwrap_or(Class::I64);
                        let v = vals[src.index()];
                        let r = if *sign { signed(from, v) as u128 } else { v };
                        vals[dst.index()] = norm(*to, r);
                    }
                    Inst::Un { dst, op, a } => {
                        let cl = classes.get(a.index()).copied().unwrap_or(Class::I64);
                        vals[dst.index()] = un_op(*op, cl, vals[a.index()]);
                    }
                    Inst::Conv { dst, op, to, src } => {
                        let from = classes.get(src.index()).copied().unwrap_or(Class::I64);
                        vals[dst.index()] = conv_op(*op, from, *to, vals[src.index()]);
                    }
                    Inst::Trunc { dst, to, src } => {
                        vals[dst.index()] = norm(*to, vals[src.index()]);
                    }
                    Inst::PtrAdd { dst, base, offset } => {
                        vals[dst.index()] = norm(
                            Class::Ptr,
                            vals[base.index()].wrapping_add(vals[offset.index()]),
                        );
                    }
                    Inst::PtrToInt { dst, src } | Inst::IntToPtr { dst, src } => {
                        vals[dst.index()] = vals[src.index()];
                    }
                    Inst::GetVar { dst, var } => vals[dst.index()] = vars[var.0 as usize],
                    Inst::SetVar { var, src } => vars[var.0 as usize] = vals[src.index()],
                    Inst::Call {
                        dst,
                        failed,
                        callee,
                        args,
                    } => {
                        let a: Vec<u128> = args.iter().map(|v| vals[v.index()]).collect();
                        let target = self.resolved[mi].get(callee.index()).copied().flatten();
                        let (r, bit) = match target {
                            Some(Target::Code { module, function }) => {
                                self.call_failing(module, function, &a)?
                            }
                            // The runtime takes words: a 128-bit value goes to it by address.
                            Some(Target::Runtime(name)) => {
                                let words: Vec<u64> = a.iter().map(|v| *v as u64).collect();
                                (self.runtime(name, &words)?.map(u128::from), false)
                            }
                            Some(Target::Data(_)) => {
                                return Err(Stop::Defect("a call to data".to_owned()))
                            }
                            None => {
                                let name = m.symbol(*callee).map_or("?", |s| s.name.as_str());
                                return Err(Stop::Refused(format!(
                                    "a call to `{name}`, which no module defines: C's"
                                )));
                            }
                        };
                        if self.exited.is_some() {
                            self.depth -= 1;
                            return Ok((None, false));
                        }
                        if let (Some(d), Some(v)) = (dst, r) {
                            let c = m.sig(*callee).and_then(|s| s.ret).unwrap_or(Class::I64);
                            vals[d.index()] = norm(c, v);
                        }
                        if let Some(f) = failed {
                            vals[f.index()] = u128::from(bit);
                        }
                    }
                    Inst::CallIndirect {
                        dst,
                        failed,
                        addr,
                        args,
                        ..
                    } => {
                        let a: Vec<u128> = args.iter().map(|v| vals[v.index()]).collect();
                        let (r, bit) = self.call_address_failing(vals[addr.index()] as u64, &a)?;
                        if self.exited.is_some() {
                            self.depth -= 1;
                            return Ok((None, false));
                        }
                        if let (Some(d), Some(v)) = (dst, r) {
                            vals[d.index()] = v;
                        }
                        if let Some(f) = failed {
                            vals[f.index()] = u128::from(bit);
                        }
                    }
                    Inst::Copy { dst, src, size } => {
                        let bytes = self.bytes(vals[src.index()] as u64, u64::from(*size))?;
                        self.put(vals[dst.index()] as u64, &bytes)?;
                    }
                    Inst::Zero { dst, size } => {
                        let n = u64::from(*size);
                        let a = vals[dst.index()] as u64;
                        self.check(a, n)?;
                        for k in 0..n {
                            self.mem[(a + k) as usize] = 0;
                        }
                    }
                    Inst::Loc(_) => {}
                }
            }
            match &block.term {
                Term::Jump(t) => b = t.index(),
                Term::Branch {
                    cond,
                    then,
                    otherwise,
                } => {
                    b = if vals[cond.index()] & 1 == 1 {
                        then.index()
                    } else {
                        otherwise.index()
                    };
                }
                Term::Switch {
                    value,
                    cases,
                    default,
                } => {
                    let v = vals[value.index()] as i64;
                    b = cases
                        .iter()
                        .find(|(k, _)| *k == v)
                        .map_or(default.index(), |(_, t)| t.index());
                }
                Term::Return(v) => {
                    self.depth -= 1;
                    return Ok((v.map(|v| vals[v.index()]), false));
                }
                // The failure exit: only after the failure is recorded, and a zero for a result.
                Term::Fail => {
                    self.depth -= 1;
                    if self.failed.is_none() && self.exited.is_none() {
                        return Err(Stop::Defect(
                            "a failure exit with no failure recorded".to_owned(),
                        ));
                    }
                    let zero = m.sig(f.sym).and_then(|s| s.ret).map(|_| 0u128);
                    return Ok((zero, true));
                }
                Term::Unreachable => {
                    self.depth -= 1;
                    if self.exited.is_some() {
                        return Ok((None, false));
                    }
                    return Err(Stop::Defect("control reached `unreachable`".to_owned()));
                }
            }
        }
    }

    // --- the runtime's services --------------------------------------------------------

    fn fail(&mut self, msg: Vec<u8>) {
        if self.failed.is_none() {
            self.failed = Some(msg);
        }
    }

    fn release_count(&mut self, a: u64) -> bool {
        if a == 0 {
            return false;
        }
        let c = self.counts.entry(a).or_insert(1);
        *c -= 1;
        *c <= 0
    }

    fn retain(&mut self, a: u64) {
        if a != 0 {
            *self.counts.entry(a).or_insert(1) += 1;
        }
    }

    fn header(&mut self) -> u64 {
        let h = self.alloc(32);
        self.counts.insert(h, 1);
        h
    }

    /// Write a string value `{ pointer, length, owner }` at `out`.
    fn write_str(&mut self, out: u64, p: u64, n: u64, owner: u64) -> Result<(), Stop> {
        self.write(out, 8, p)?;
        self.write(out + 8, 8, n)?;
        self.write(out + 16, 8, owner)
    }

    fn backing(&mut self, bytes: &[u8]) -> Result<(u64, u64), Stop> {
        let owner = self.alloc(rt::STRING_HEADER as u64 + bytes.len() as u64 + 1);
        self.counts.insert(owner, 1);
        let data = owner + rt::STRING_HEADER as u64;
        self.put(data, bytes)?;
        Ok((owner, data))
    }

    #[allow(clippy::too_many_lines)]
    fn runtime(&mut self, name: &'static str, a: &[u64]) -> Result<Option<u64>, Stop> {
        let arg = |i: usize| a.get(i).copied().unwrap_or(0);
        Ok(match name {
            "nz.fail" => {
                let msg = self.bytes(arg(0), arg(1))?;
                self.fail(msg);
                None
            }
            "nz.failed_now" => Some(u64::from(self.failed.is_some())),
            "nz.report" => {
                if let Some(m) = self.failed.take() {
                    self.stderr.extend_from_slice(&m);
                }
                self.exited = Some(2);
                None
            }
            "nz.stack_check" | "nz.stack_check_frame" => Some(u64::from(self.depth > DEPTH - 100)),
            // The oracle is one thread: nothing ever asks it to yield (6.3).
            "nz.preempt" => None,
            // The oracle is one thread (Gate 3): an atomic access is the plain one.
            "nz.atomic_load" => Some(self.read(arg(0), 8)?),
            "nz.atomic_store" => {
                self.write(arg(0), 8, arg(1))?;
                None
            }
            "nz.atomic_swap" => {
                let old = self.read(arg(0), 8)?;
                self.write(arg(0), 8, arg(1))?;
                Some(old)
            }
            "nz.atomic_cas" => {
                let held = self.read(arg(0), 8)? == arg(1);
                if held {
                    self.write(arg(0), 8, arg(2))?;
                }
                Some(u64::from(held))
            }
            "nz.closure_new" => {
                let c = self.alloc(arg(0));
                self.write(c, 8, 1)?;
                self.write(c + 8, 8, arg(1))?;
                self.write(c + 16, 8, arg(2))?;
                self.counts.insert(c, 1);
                Some(c)
            }
            "nz.closure_retain" | "nz.str_retain" | "nz.arr_retain" | "nz.chan_retain" => {
                self.retain(arg(0));
                None
            }
            "nz.closure_release" => {
                let c = arg(0);
                if self.release_count(c) {
                    let release = self.read(c + 16, 8)?;
                    if release != 0 {
                        self.call_address(release, &[u128::from(c)])?;
                    }
                }
                None
            }
            "nz.format_int" => {
                let text = (arg(1) as i64).to_string();
                self.put(arg(0), text.as_bytes())?;
                Some(text.len() as u64)
            }
            // A float's text and an unsigned word's (v1, Q1-B-07): by the one function the
            // interpreter and the specification use, `nazm_sema::numeric::to_str`, so the oracle
            // agrees with them by construction rather than by a second implementation.
            "nz.format_float" => {
                let kind = if arg(2) & 1 == 1 {
                    nazm_sema::NumKind::F32
                } else {
                    nazm_sema::NumKind::F64
                };
                let x = f64::from_bits(arg(1));
                #[allow(clippy::cast_possible_truncation)]
                let bits = match kind {
                    nazm_sema::NumKind::F32 => u128::from((x as f32).to_bits()),
                    _ => u128::from(arg(1)),
                };
                let text = nazm_sema::numeric::to_str(nazm_sema::numeric::NumVal { kind, bits });
                self.put(arg(0), text.as_bytes())?;
                Some(text.len() as u64)
            }
            // A decimal float's value and its grammar (Gate 2): the interpreter's functions.
            "nz.float_parse" => {
                let b = self.bytes(arg(0), arg(1))?;
                Some(nazm_sema::numeric::parse(&String::from_utf8_lossy(&b)).to_bits())
            }
            "nz.float_parses" => {
                let b = self.bytes(arg(0), arg(1))?;
                Some(u64::from(nazm_sema::numeric::parses(
                    &String::from_utf8_lossy(&b),
                )))
            }
            // 128-bit integers (R1b, L13b), by address, as the runtime takes them.
            "nz.i128_divrem" => {
                let (x, y) = (self.read_wide(arg(1), 16)?, self.read_wide(arg(2), 16)?);
                if y == 0 {
                    return Err(Stop::Defect(
                        "a 128-bit division LIR did not guard".to_owned(),
                    ));
                }
                let (sx, sy) = (x as i128, y as i128);
                let r = match arg(3) {
                    0 => sx.wrapping_div(sy) as u128,
                    1 => sx.wrapping_rem(sy) as u128,
                    2 => x / y,
                    _ => x % y,
                };
                self.write_wide(arg(0), 16, r)?;
                None
            }
            "nz.i128_mul" => {
                let (x, y) = (self.read_wide(arg(1), 16)?, self.read_wide(arg(2), 16)?);
                self.write_wide(arg(0), 16, x.wrapping_mul(y))?;
                let over = if arg(3) != 0 {
                    (x as i128).checked_mul(y as i128).is_none()
                } else {
                    x.checked_mul(y).is_none()
                };
                Some(u64::from(over))
            }
            // Rounded once, as the runtime's `sitofp`/`uitofp` round.
            #[allow(clippy::cast_precision_loss)]
            "nz.i128_to_f64" | "nz.i128_to_f32" => {
                let x = self.read_wide(arg(0), 16)?;
                let signed = arg(1) != 0;
                Some(if name == "nz.i128_to_f32" {
                    u64::from(if signed { x as i128 as f32 } else { x as f32 }.to_bits())
                } else if signed {
                    (x as i128 as f64).to_bits()
                } else {
                    (x as f64).to_bits()
                })
            }
            "nz.f64_to_i128" => {
                // Rust's casts saturate, a NaN to 0, as the runtime's `fptosi.sat` does.
                let x = f64::from_bits(arg(1));
                let r = if arg(2) != 0 {
                    x as i128 as u128
                } else {
                    x as u128
                };
                self.write_wide(arg(0), 16, r)?;
                None
            }
            "nz.format_i128" => {
                let x = self.read_wide(arg(1), 16)?;
                let text = if arg(2) != 0 {
                    (x as i128).to_string()
                } else {
                    x.to_string()
                };
                self.put(arg(0), text.as_bytes())?;
                Some(text.len() as u64)
            }
            // Fixed-point (R1a, L4): the interpreter's function too.
            "nz.format_fixed" => {
                let x = f64::from_bits(arg(1));
                let digits = usize::try_from(arg(2)).unwrap_or(0);
                let text = nazm_sema::numeric::to_fixed(
                    nazm_sema::numeric::NumVal {
                        kind: nazm_sema::NumKind::F64,
                        bits: u128::from(x.to_bits()),
                    },
                    digits,
                );
                self.put(arg(0), text.as_bytes())?;
                Some(text.len() as u64)
            }
            "nz.format_u64" => {
                let text = arg(1).to_string();
                self.put(arg(0), text.as_bytes())?;
                Some(text.len() as u64)
            }
            "nz.str_alloc" => {
                let n = arg(0);
                if n > (1 << 40) {
                    return Ok(Some(0));
                }
                let owner = self.alloc(n + rt::STRING_HEADER as u64);
                self.counts.insert(owner, 1);
                Some(owner)
            }
            "nz.str_release" => {
                self.release_count(arg(0));
                None
            }
            "nz.seq_new" | "nz.arr_new" => Some(self.header()),
            "nz.seq_release" | "nz.chan_release" => {
                self.release_count(arg(0));
                None
            }
            "nz.arr_release" => Some(u64::from(self.release_count(arg(0)))),
            "nz.strs_release" => {
                let h = arg(0);
                if self.release_count(h) {
                    let len = self.read(h, 8)?;
                    let data = self.read(h + 16, 8)?;
                    for i in 0..len {
                        let owner = self.read(data + 24 * i + 16, 8)?;
                        self.release_count(owner);
                    }
                }
                None
            }
            // The runtime's queued release (R1a): nested in an element's release (a queue, any
            // non-zero word here), the sequence is queued; outermost, it is emptied and then
            // every sequence queued meanwhile, in a loop.
            "nz.vec_release" => {
                let (h, f, stride, q) = (arg(0), arg(1), arg(2), arg(3));
                if self.release_count(h) {
                    if q != 0 {
                        self.releasing.push((h, f, stride));
                    } else {
                        let mark = self.releasing.len();
                        let mut next = Some((h, f, stride));
                        while let Some((h, f, stride)) = next {
                            let len = self.read(h, 8)?;
                            let data = self.read(h + 16, 8)?;
                            for i in 0..len {
                                self.call_address(f, &[u128::from(data + stride * i), 1])?;
                            }
                            next = if self.releasing.len() > mark {
                                self.releasing.pop()
                            } else {
                                None
                            };
                        }
                    }
                }
                None
            }
            "nz.arr_grow" => {
                let (h, elem) = (arg(0), arg(1));
                let len = self.read(h, 8)?;
                let cap = self.read(h + 8, 8)?;
                if len >= cap {
                    let want = (cap * 2).max(4);
                    let Some(bytes) = want.checked_mul(elem).filter(|b| *b < (1 << 40)) else {
                        return Ok(Some(0));
                    };
                    let old = self.read(h + 16, 8)?;
                    let new = self.alloc(bytes);
                    if old != 0 {
                        let keep = self.bytes(old, len * elem)?;
                        self.put(new, &keep)?;
                    }
                    self.write(h + 16, 8, new)?;
                    self.write(h + 8, 8, want)?;
                }
                Some(self.read(h + 16, 8)?)
            }
            "nz.soa_grow" => {
                let (h, nf) = (arg(0), arg(1));
                let len = self.read(h, 8)?;
                let cap = self.read(h + 8, 8)?;
                if len >= cap {
                    let want = (cap * 2).max(4);
                    let old = self.read(h + 16, 8)?;
                    let new = self.alloc(nf * want * 8);
                    for k in 0..nf {
                        if old != 0 {
                            let keep = self.bytes(old + k * cap * 8, len * 8)?;
                            self.put(new + k * want * 8, &keep)?;
                        }
                    }
                    self.write(h + 16, 8, new)?;
                    self.write(h + 8, 8, want)?;
                }
                Some(self.read(h + 16, 8)?)
            }
            "nz.ints_sum" => {
                let (h, start, flag) = (arg(0), arg(1), arg(2));
                let len = self.read(h, 8)?;
                let data = self.read(h + 16, 8)?;
                self.write(flag, 4, 0)?;
                let mut s = start as i64;
                for i in 0..len {
                    let x = self.read(data + 8 * i, 8)? as i64;
                    match s.checked_add(x) {
                        Some(n) => s = n,
                        None => {
                            self.write(flag, 4, 1)?;
                            return Ok(Some(0));
                        }
                    }
                }
                Some(s as u64)
            }
            "nz.read_file_to" => {
                let path = self.bytes(arg(1), arg(2))?;
                let out = arg(0);
                match std::fs::read(String::from_utf8_lossy(&path).as_ref()) {
                    Ok(bytes) => {
                        let (owner, data) = self.backing(&bytes)?;
                        self.write_str(out, data, bytes.len() as u64, owner)?;
                    }
                    Err(_) => self.write_str(out, 0, 0, 0)?,
                }
                None
            }
            "nz.write_file" => {
                let path = self.bytes(arg(0), arg(1))?;
                let data = self.bytes(arg(2), arg(3))?;
                match std::fs::write(String::from_utf8_lossy(&path).as_ref(), &data) {
                    Ok(()) => Some(data.len() as u64),
                    Err(_) => Some(-1i64 as u64),
                }
            }
            "nz.file_exists" => {
                let path = self.bytes(arg(0), arg(1))?;
                Some(u64::from(
                    std::path::Path::new(String::from_utf8_lossy(&path).as_ref()).exists(),
                ))
            }
            "nz.str_join_to" => {
                let (out, h, sp, sn) = (arg(0), arg(1), arg(2), arg(3));
                let sep = self.bytes(sp, sn)?;
                let len = self.read(h, 8)?;
                let data = self.read(h + 16, 8)?;
                let mut joined = Vec::new();
                for i in 0..len {
                    if i > 0 {
                        joined.extend_from_slice(&sep);
                    }
                    let p = self.read(data + 24 * i, 8)?;
                    let n = self.read(data + 24 * i + 8, 8)?;
                    joined.extend(self.bytes(p, n)?);
                }
                let (owner, d) = self.backing(&joined)?;
                self.write_str(out, d, joined.len() as u64, owner)?;
                None
            }
            "nz.scope_join_to" => {
                let (h, out) = (arg(0), arg(1));
                if self.read(h, 8)? != 0 {
                    return Err(Stop::Refused("a scope with tasks".to_owned()));
                }
                self.write(out, 8, 0)?;
                self.write(out + 8, 8, 0)?;
                None
            }
            "nz.sched_note_join" | "nz.sched_note_spawn" => None,
            "nz.cstr_has_nul" => {
                let b = self.bytes(arg(0), arg(1))?;
                Some(u64::from(b.contains(&0)))
            }
            "nz.cstr_borrow" => {
                let b = self.bytes(arg(0), arg(1))?;
                let p = self.alloc(b.len() as u64 + 1);
                self.put(p, &b)?;
                Some(p)
            }
            "nz.cstr_len" | "strlen" => {
                let mut n = 0;
                while self.read(arg(0) + n, 1)? != 0 {
                    n += 1;
                }
                Some(n)
            }
            "write" => {
                let b = self.bytes(arg(1), arg(2))?;
                match arg(0) as u32 {
                    1 => self.stdout.extend_from_slice(&b),
                    2 => self.stderr.extend_from_slice(&b),
                    _ => return Ok(Some(-1i64 as u64)),
                }
                Some(b.len() as u64)
            }
            "exit" => {
                self.exited = Some(arg(0) as u32 as i32);
                None
            }
            "malloc" => Some(self.alloc(arg(0))),
            "free" => None,
            "memcpy" => {
                let b = self.bytes(arg(1), arg(2))?;
                self.put(arg(0), &b)?;
                Some(arg(0))
            }
            "memcmp" => {
                let x = self.bytes(arg(0), arg(2))?;
                let y = self.bytes(arg(1), arg(2))?;
                Some(match x.cmp(&y) {
                    std::cmp::Ordering::Less => -1i32 as u32 as u64,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })
            }
            // Bytes and text (Gate 2), by the host's UTF-8: the oracle the runtime's text is
            // held to.
            "nz.utf8_valid" => {
                let b = self.bytes(arg(0), arg(1))?;
                Some(u64::from(std::str::from_utf8(&b).is_ok()))
            }
            "nz.utf8_count" => {
                let b = self.bytes(arg(0), arg(1))?;
                Some(b.iter().filter(|&&x| x & 0xC0 != 0x80).count() as u64)
            }
            "nz.utf8_decode" => {
                let b = self.bytes(arg(0), arg(1))?;
                let at = usize::try_from(arg(2)).unwrap_or(usize::MAX);
                let c = b
                    .get(at..)
                    .and_then(|rest| {
                        let n = rest.len().min(4);
                        (1..=n).find_map(|k| std::str::from_utf8(&rest[..k]).ok())
                    })
                    .and_then(|s| s.chars().next())
                    .map_or(0, u64::from);
                Some(c)
            }
            "nz.utf8_encode" => {
                let c = u32::try_from(arg(1))
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\u{fffd}');
                let mut buf = [0u8; 4];
                let e = c.encode_utf8(&mut buf);
                self.put(arg(0), e.as_bytes())?;
                Some(e.len() as u64)
            }
            "nz.mem_zero" => {
                let n = usize::try_from(arg(1)).unwrap_or(0);
                self.put(arg(0), &vec![0u8; n])?;
                None
            }
            "nz.mem_move" => {
                let b = self.bytes(arg(1), arg(2))?;
                self.put(arg(0), &b)?;
                None
            }
            other => return Err(Stop::Refused(format!("the runtime's `{other}`"))),
        })
    }
}

/// A value of `class`, as the oracle holds it: a bit is 0 or 1, a narrow integer zero-extended.
fn norm(class: Class, v: u128) -> u128 {
    match class {
        Class::Bit => v & 1,
        Class::I8 => v & 0xff,
        Class::I16 => v & 0xffff,
        Class::I32 | Class::F32 => v & 0xffff_ffff,
        Class::I64 | Class::F64 | Class::Ptr => v & u128::from(u64::MAX),
        Class::I128 => v,
    }
}

/// A value of an integer class read as signed.
fn signed(class: Class, v: u128) -> i128 {
    match class {
        Class::Bit => (v & 1) as i128,
        Class::I8 => i128::from(v as u8 as i8),
        Class::I16 => i128::from(v as u16 as i16),
        Class::I32 => i128::from(v as u32 as i32),
        Class::I128 => v as i128,
        _ => i128::from(v as u64 as i64),
    }
}

fn f32_of(v: u128) -> f32 {
    f32::from_bits(v as u32)
}

fn f64_of(v: u128) -> f64 {
    f64::from_bits(v as u64)
}

/// A wrapping or float operation on two values of `class` (Gate 2 widened this to every class).
fn bin_op(op: BinOp, class: Class, x: u128, y: u128) -> Result<u128, Stop> {
    let w = class.bits();
    let unguarded = |what: &str| Err(Stop::Defect(format!("a {what} LIR did not guard")));
    let float = |f: fn(f64, f64) -> f64, g: fn(f32, f32) -> f32| {
        if class == Class::F32 {
            u128::from(g(f32_of(x), f32_of(y)).to_bits())
        } else {
            u128::from(f(f64_of(x), f64_of(y)).to_bits())
        }
    };
    let r = match op {
        BinOp::Add => x.wrapping_add(y),
        BinOp::Sub => x.wrapping_sub(y),
        BinOp::Mul => x.wrapping_mul(y),
        BinOp::SDiv | BinOp::SRem => {
            let (a, b) = (signed(class, x), signed(class, y));
            if b == 0 {
                return unguarded("division by zero");
            }
            (if op == BinOp::SDiv {
                a.wrapping_div(b)
            } else {
                a.wrapping_rem(b)
            }) as u128
        }
        BinOp::UDiv | BinOp::URem => {
            let (a, b) = (norm(class, x), norm(class, y));
            if b == 0 {
                return unguarded("division by zero");
            }
            if op == BinOp::UDiv {
                a / b
            } else {
                a % b
            }
        }
        BinOp::And => x & y,
        BinOp::Or => x | y,
        BinOp::Xor => x ^ y,
        BinOp::Shl | BinOp::LShr | BinOp::AShr => {
            let n = norm(class, y);
            if n >= u128::from(w) {
                return unguarded("shift out of range");
            }
            let n = n as u32;
            match op {
                BinOp::Shl => x << n,
                BinOp::LShr => norm(class, x) >> n,
                _ => (signed(class, x) >> n) as u128,
            }
        }
        BinOp::Rotl | BinOp::Rotr => {
            let v = norm(class, x);
            let n = (norm(class, y) % u128::from(w)) as u32;
            if n == 0 {
                v
            } else if op == BinOp::Rotl {
                (v << n) | (v >> (w - n))
            } else {
                (v >> n) | (v << (w - n))
            }
        }
        BinOp::FAdd => float(|a, b| a + b, |a, b| a + b),
        BinOp::FSub => float(|a, b| a - b, |a, b| a - b),
        BinOp::FMul => float(|a, b| a * b, |a, b| a * b),
        BinOp::FDiv => float(|a, b| a / b, |a, b| a / b),
    };
    Ok(norm(class, r))
}

/// An overflow-checked operation on two values of an integer class: the wrapped result, and
/// whether the exact one left the class's signed or unsigned range.
fn ovf_op(op: OvfOp, class: Class, x: u128, y: u128) -> (u128, bool) {
    let w = class.bits();
    let (lo, hi) = if w == 128 {
        (i128::MIN, i128::MAX)
    } else {
        (-(1i128 << (w - 1)), (1i128 << (w - 1)) - 1)
    };
    let umax = if w == 128 {
        u128::MAX
    } else {
        (1u128 << w) - 1
    };
    let (sa, sb) = (signed(class, x), signed(class, y));
    let (ua, ub) = (norm(class, x), norm(class, y));
    let s = |exact: Option<i128>| exact.is_some_and(|v| (lo..=hi).contains(&v));
    let u = |exact: Option<u128>| exact.is_some_and(|v| v <= umax);
    let (r, ok) = match op {
        OvfOp::Add => (sa.wrapping_add(sb) as u128, s(sa.checked_add(sb))),
        OvfOp::Sub => (sa.wrapping_sub(sb) as u128, s(sa.checked_sub(sb))),
        OvfOp::Mul => (sa.wrapping_mul(sb) as u128, s(sa.checked_mul(sb))),
        OvfOp::UAdd => (ua.wrapping_add(ub), u(ua.checked_add(ub))),
        OvfOp::USub => (ua.wrapping_sub(ub), ua >= ub),
        OvfOp::UMul => (ua.wrapping_mul(ub), u(ua.checked_mul(ub))),
    };
    (norm(class, r), !ok)
}

/// A comparison of two values of `class`.
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn cmp_op(op: Cmp, class: Class, x: u128, y: u128) -> bool {
    let (sa, sb) = (signed(class, x), signed(class, y));
    let (ua, ub) = (norm(class, x), norm(class, y));
    let (fa, fb) = if class == Class::F32 {
        (f64::from(f32_of(x)), f64::from(f32_of(y)))
    } else {
        (f64_of(x), f64_of(y))
    };
    match op {
        Cmp::Eq => ua == ub,
        Cmp::Ne => ua != ub,
        Cmp::Lt => sa < sb,
        Cmp::Le => sa <= sb,
        Cmp::Gt => sa > sb,
        Cmp::Ge => sa >= sb,
        Cmp::ULt => ua < ub,
        Cmp::ULe => ua <= ub,
        Cmp::UGt => ua > ub,
        Cmp::UGe => ua >= ub,
        Cmp::FEq => fa == fb,
        Cmp::FNe => fa != fb,
        Cmp::FLt => fa < fb,
        Cmp::FLe => fa <= fb,
        Cmp::FGt => fa > fb,
        Cmp::FGe => fa >= fb,
    }
}

/// An operation of one operand.
fn un_op(op: UnOp, class: Class, x: u128) -> u128 {
    let w = class.bits();
    let float = |f: fn(f64) -> f64, g: fn(f32) -> f32| {
        if class == Class::F32 {
            u128::from(g(f32_of(x)).to_bits())
        } else {
            u128::from(f(f64_of(x)).to_bits())
        }
    };
    let v = norm(class, x);
    norm(
        class,
        match op {
            UnOp::FNeg => float(|a| -a, |a| -a),
            UnOp::FSqrt => float(f64::sqrt, f32::sqrt),
            UnOp::FAbs => float(f64::abs, f32::abs),
            UnOp::FFloor => float(f64::floor, f32::floor),
            UnOp::FCeil => float(f64::ceil, f32::ceil),
            UnOp::FTrunc => float(f64::trunc, f32::trunc),
            UnOp::Popcnt => u128::from(v.count_ones()),
            UnOp::Clz => u128::from(v.leading_zeros() - (128 - w)),
            UnOp::Ctz => {
                if v == 0 {
                    u128::from(w)
                } else {
                    u128::from(v.trailing_zeros())
                }
            }
            UnOp::Bswap => v.swap_bytes() >> (128 - w),
        },
    )
}

/// A conversion between classes.
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn conv_op(op: ConvOp, from: Class, to: Class, x: u128) -> u128 {
    let fval = || {
        if from == Class::F32 {
            f64::from(f32_of(x))
        } else {
            f64_of(x)
        }
    };
    let float_out = |v: f64| {
        if to == Class::F32 {
            u128::from((v as f32).to_bits())
        } else {
            u128::from(v.to_bits())
        }
    };
    let r = match op {
        // Directly to `f32` from the integer, so it is rounded once.
        ConvOp::SToF if to == Class::F32 => u128::from((signed(from, x) as f32).to_bits()),
        ConvOp::UToF if to == Class::F32 => u128::from((norm(from, x) as f32).to_bits()),
        ConvOp::SToF => float_out(signed(from, x) as f64),
        ConvOp::UToF => float_out(norm(from, x) as f64),
        ConvOp::FToS if to == Class::I128 => fval() as i128 as u128,
        ConvOp::FToU if to == Class::I128 => fval() as u128,
        ConvOp::FToS => u128::from(fval() as i64 as u64),
        ConvOp::FToU => u128::from(fval() as u64),
        ConvOp::FExt | ConvOp::FTrunc => float_out(fval()),
        ConvOp::Bits => x,
    };
    norm(to, r)
}

/// The runtime names the oracle models.
fn runtime_name(name: &str) -> Option<&'static str> {
    crate::runtime::import(name)
        .map(|i| i.symbol)
        .or(match name {
            "nz.argc" => Some("nz.argc"),
            "nz.argv" => Some("nz.argv"),
            _ => None,
        })
}

/// Every value's class, read off its definition: passes until nothing changes, since a value
/// defined from another (an operation's result) takes its operand's class.
fn value_classes(f: &super::ir::Function, m: &Module) -> Vec<Class> {
    let mut c = vec![Class::I64; f.values as usize];
    for _ in 0..4 {
        for b in &f.blocks {
            for i in &b.insts {
                let mut set = |v: super::ir::V, k: Class| {
                    if let Some(x) = c.get_mut(v.index()) {
                        *x = k;
                    }
                };
                match i {
                    Inst::Param { dst, index } => {
                        let k = m
                            .sig(f.sym)
                            .and_then(|s| s.params.get(*index as usize).copied())
                            .unwrap_or(Class::I64);
                        set(*dst, k);
                    }
                    Inst::Const { dst, class, .. } | Inst::Load { dst, class, .. } => {
                        set(*dst, *class);
                    }
                    Inst::Ext { dst, to, .. } | Inst::Trunc { dst, to, .. } => set(*dst, *to),
                    Inst::Cmp { dst, .. } => set(*dst, Class::Bit),
                    Inst::Ovf { dst, ovf, a, .. } => {
                        let k = c.get(a.index()).copied().unwrap_or(Class::I64);
                        if let Some(x) = c.get_mut(dst.index()) {
                            *x = k;
                        }
                        if let Some(x) = c.get_mut(ovf.index()) {
                            *x = Class::Bit;
                        }
                    }
                    Inst::Conv { dst, to, .. } => set(*dst, *to),
                    Inst::Addr { dst, .. }
                    | Inst::SlotAddr { dst, .. }
                    | Inst::PtrAdd { dst, .. }
                    | Inst::IntToPtr { dst, .. } => set(*dst, Class::Ptr),
                    Inst::PtrToInt { dst, .. } => set(*dst, Class::I64),
                    Inst::GetVar { dst, var } => {
                        set(
                            *dst,
                            f.vars.get(var.0 as usize).copied().unwrap_or(Class::I64),
                        );
                    }
                    Inst::Bin { dst, a, .. }
                    | Inst::Select { dst, a, .. }
                    | Inst::Un { dst, a, .. } => {
                        let k = c.get(a.index()).copied().unwrap_or(Class::I64);
                        if let Some(x) = c.get_mut(dst.index()) {
                            *x = k;
                        }
                    }
                    Inst::Call {
                        dst,
                        failed,
                        callee,
                        ..
                    } => {
                        if let Some(d) = dst {
                            let k = m.sig(*callee).and_then(|s| s.ret).unwrap_or(Class::I64);
                            set(*d, k);
                        }
                        if let Some(f) = failed {
                            set(*f, Class::Bit);
                        }
                    }
                    Inst::CallIndirect {
                        dst, failed, sig, ..
                    } => {
                        if let Some(d) = dst {
                            set(*d, sig.ret.unwrap_or(Class::I64));
                        }
                        if let Some(f) = failed {
                            set(*f, Class::Bit);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    c
}
