//! The rules every backend relies on, checked before either sees a module (N105).
//!
//! Every value is defined once, before its use in its block or in a block that dominates the
//! use; every class agrees with what consumes it; every block, variable, slot and symbol exists;
//! every call matches its callee's signature; every function's returns match its own. A module
//! that breaks one is a compiler defect, named.

use std::collections::BTreeMap;

use super::ir::{BinOp, Class, Function, Inst, Module, Sig, SymKind, Term, B, V};

/// Check a module.
///
/// # Errors
///
/// Every broken rule, each naming its function and where in it.
pub fn module(m: &Module) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for d in &m.data {
        if !matches!(m.symbol(d.sym).map(|s| &s.kind), Some(SymKind::Data { .. })) {
            errors.push(format!("data at symbol {} is not a data symbol", d.sym.0));
        }
        for (o, s) in &d.relocs {
            if m.symbol(*s).is_none() {
                errors.push(format!("data {} refers to no symbol at {o}", d.sym.0));
            }
        }
    }
    for f in &m.functions {
        if let Err(mut e) = function(m, f) {
            let name = m.symbol(f.sym).map_or("?", |s| s.name.as_str());
            for x in &mut e {
                *x = format!("`{name}`: {x}");
            }
            errors.extend(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn successors(t: &Term) -> Vec<B> {
    match t {
        Term::Jump(b) => vec![*b],
        Term::Branch {
            then, otherwise, ..
        } => vec![*then, *otherwise],
        Term::Switch { cases, default, .. } => {
            let mut v: Vec<B> = cases.iter().map(|(_, b)| *b).collect();
            v.push(*default);
            v
        }
        Term::Return(_) | Term::Fail | Term::Unreachable => Vec::new(),
    }
}

/// Each block's immediate dominator, `None` where it is unreachable: Cooper, Harvey and Kennedy's
/// iteration over reverse postorder. Near-linear in practice, where the bit-vector form it replaced
/// was cubic in a function's blocks and dominated the build of the compiler written in Nazm (N105).
fn dominators(f: &Function) -> Vec<Option<usize>> {
    let n = f.blocks.len();
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Postorder by an explicit stack: (block, whether its successors have been pushed).
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut stack = vec![(0usize, false)];
    while let Some((b, done)) = stack.pop() {
        if done {
            order.push(b);
            continue;
        }
        let (Some(block), Some(mark)) = (f.blocks.get(b), seen.get_mut(b)) else {
            continue;
        };
        if *mark {
            continue;
        }
        *mark = true;
        stack.push((b, true));
        for s in successors(&block.term) {
            let s = s.index();
            if let Some(p) = preds.get_mut(s) {
                p.push(b);
                if !seen.get(s).copied().unwrap_or(true) {
                    stack.push((s, false));
                }
            }
        }
    }
    let mut rpo_no = vec![usize::MAX; n];
    for (k, b) in order.iter().rev().enumerate() {
        if let Some(r) = rpo_no.get_mut(*b) {
            *r = k;
        }
    }
    let rpo = |b: usize| rpo_no.get(b).copied().unwrap_or(usize::MAX);
    let mut idom: Vec<Option<usize>> = vec![None; n];
    if let Some(entry) = idom.first_mut() {
        *entry = Some(0);
    }
    let up = |idom: &[Option<usize>], b: usize| idom.get(b).copied().flatten().unwrap_or(0);
    let intersect = |idom: &[Option<usize>], mut a: usize, mut b: usize| {
        while a != b {
            while rpo(a) > rpo(b) {
                a = up(idom, a);
            }
            while rpo(b) > rpo(a) {
                b = up(idom, b);
            }
        }
        a
    };
    let mut changed = true;
    while changed {
        changed = false;
        for &b in order.iter().rev().skip(1) {
            let mut new: Option<usize> = None;
            for &p in preds.get(b).map_or(&[][..], Vec::as_slice) {
                if idom.get(p).copied().flatten().is_none() {
                    continue;
                }
                new = Some(new.map_or(p, |x| intersect(&idom, p, x)));
            }
            if let Some(slot) = idom.get_mut(b) {
                if new.is_some() && *slot != new {
                    *slot = new;
                    changed = true;
                }
            }
        }
    }
    idom
}

/// Whether block `a` dominates block `b`, both reachable.
fn dominates(idom: &[Option<usize>], a: usize, b: usize) -> bool {
    let mut x = b;
    loop {
        if x == a {
            return true;
        }
        match idom.get(x).copied().flatten() {
            Some(up) if up != x => x = up,
            _ => return false,
        }
    }
}

fn function(m: &Module, f: &Function) -> Result<(), Vec<String>> {
    let mut e = Vec::new();
    let Some(own) = m.sig(f.sym).cloned() else {
        return Err(vec![
            "defined under a symbol that is not a function".to_owned()
        ]);
    };
    let n = f.blocks.len();
    if n == 0 {
        return Err(vec!["no blocks".to_owned()]);
    }
    // Where each value is defined, and its class.
    let mut def: BTreeMap<V, (usize, usize, Class)> = BTreeMap::new();
    let dom = dominators(f);
    let class_of = |def: &BTreeMap<V, (usize, usize, Class)>, v: V| def.get(&v).map(|d| d.2);
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            let mut defs: Vec<(V, Class, usize)> = Vec::new();
            let known = |v: V| def.get(&v).map(|d| d.2);
            let mut define = |e: &mut Vec<String>, v: V, c: Class| {
                defs.push((v, c, e.len()));
                let _ = e;
            };
            match i {
                Inst::Param { dst, index } => {
                    if bi != 0 {
                        e.push(format!("a parameter outside the entry block, b{bi}"));
                    }
                    match own.params.get(*index as usize) {
                        Some(c) => define(&mut e, *dst, *c),
                        None => e.push(format!("parameter {index} does not exist")),
                    }
                }
                Inst::Const { dst, class, .. } => define(&mut e, *dst, *class),
                Inst::Addr { dst, sym } => {
                    if m.symbol(*sym).is_none() {
                        e.push(format!("the address of no symbol, {}", sym.0));
                    }
                    define(&mut e, *dst, Class::Ptr);
                }
                Inst::SlotAddr { dst, slot } => {
                    if slot.0 as usize >= f.slots.len() {
                        e.push(format!("no slot s{}", slot.0));
                    }
                    define(&mut e, *dst, Class::Ptr);
                }
                Inst::Load { dst, class, .. } => define(&mut e, *dst, *class),
                Inst::Bin { dst, a, .. } => {
                    let c = known(*a).unwrap_or(Class::I64);
                    define(&mut e, *dst, c);
                }
                Inst::Ovf { dst, ovf, a, .. } => {
                    let c = known(*a).unwrap_or(Class::I64);
                    define(&mut e, *dst, c);
                    define(&mut e, *ovf, Class::Bit);
                }
                Inst::Un { dst, a, .. } => {
                    let c = known(*a).unwrap_or(Class::I64);
                    define(&mut e, *dst, c);
                }
                Inst::Conv { dst, to, .. } => define(&mut e, *dst, *to),
                Inst::Cmp { dst, .. } => define(&mut e, *dst, Class::Bit),
                Inst::Select { dst, a, .. } => {
                    let c = known(*a).unwrap_or(Class::I64);
                    define(&mut e, *dst, c);
                }
                Inst::Ext { dst, to, .. } | Inst::Trunc { dst, to, .. } => {
                    define(&mut e, *dst, *to);
                }
                Inst::PtrAdd { dst, .. } | Inst::IntToPtr { dst, .. } => {
                    define(&mut e, *dst, Class::Ptr);
                }
                Inst::PtrToInt { dst, .. } => define(&mut e, *dst, Class::I64),
                Inst::GetVar { dst, var } => match f.vars.get(var.0 as usize) {
                    Some(c) => define(&mut e, *dst, *c),
                    None => e.push(format!("no variable x{}", var.0)),
                },
                Inst::Call {
                    dst,
                    failed,
                    callee,
                    ..
                } => {
                    let ret = m.sig(*callee).and_then(|s| s.ret);
                    if let Some(fv) = failed {
                        if !m.sig(*callee).is_some_and(Sig::fails) {
                            e.push(format!(
                                "the failure bit of `{}`, which returns none",
                                m.symbol(*callee).map_or("?", |s| s.name.as_str())
                            ));
                        }
                        define(&mut e, *fv, Class::Bit);
                    }
                    match (dst, ret) {
                        (Some(d), Some(c)) => define(&mut e, *d, c),
                        (None, _) => {}
                        (Some(_), None) => e.push(format!(
                            "a result of `{}`, which returns nothing",
                            m.symbol(*callee).map_or("?", |s| s.name.as_str())
                        )),
                    }
                }
                Inst::CallIndirect {
                    dst, failed, sig, ..
                } => {
                    if let Some(fv) = failed {
                        if !sig.fails() {
                            e.push(
                                "the failure bit of an indirect callee that returns none"
                                    .to_owned(),
                            );
                        }
                        define(&mut e, *fv, Class::Bit);
                    }
                    match (dst, sig.ret) {
                        (Some(d), Some(c)) => define(&mut e, *d, c),
                        (None, _) => {}
                        (Some(_), None) => {
                            e.push("a result of an indirect call that returns nothing".to_owned());
                        }
                    }
                }
                Inst::Store { .. }
                | Inst::SetVar { .. }
                | Inst::Copy { .. }
                | Inst::Zero { .. }
                | Inst::Loc(_) => {}
            }
            for (v, c, _) in defs {
                if def.insert(v, (bi, ii, c)).is_some() {
                    e.push(format!("v{} is defined twice", v.0));
                }
            }
        }
    }
    failure_edges(f, &mut e);
    // Every use: defined, dominating, and of the class its consumer needs.
    for (bi, b) in f.blocks.iter().enumerate() {
        if dom.get(bi).copied().flatten().is_none() {
            continue;
        }
        let check = |e: &mut Vec<String>, v: V, at: usize, want: Option<&[Class]>| {
            let Some((db, di, c)) = def.get(&v).copied() else {
                e.push(format!("v{} is used in b{bi} and defined nowhere", v.0));
                return;
            };
            let ok = if db == bi {
                di < at
            } else {
                dominates(&dom, db, bi)
            };
            if !ok {
                e.push(format!(
                    "v{} is used in b{bi} where its definition in b{db} does not reach",
                    v.0
                ));
            }
            if let Some(w) = want {
                if !w.contains(&c) {
                    e.push(format!(
                        "v{} is a `{}` used in b{bi} where {} is needed",
                        v.0,
                        c.name(),
                        w.iter().map(|x| x.name()).collect::<Vec<_>>().join(" or ")
                    ));
                }
            }
        };
        let ints: &[Class] = &[Class::I8, Class::I16, Class::I32, Class::I64, Class::I128];
        let floats: &[Class] = &[Class::F32, Class::F64];
        let numbers: &[Class] = &[
            Class::I8,
            Class::I16,
            Class::I32,
            Class::I64,
            Class::I128,
            Class::F32,
            Class::F64,
        ];
        let ptr: &[Class] = &[Class::Ptr];
        let bit: &[Class] = &[Class::Bit];
        let i64c: &[Class] = &[Class::I64];
        for (ii, i) in b.insts.iter().enumerate() {
            match i {
                Inst::Load { addr, .. } => check(&mut e, *addr, ii, Some(ptr)),
                Inst::Store { addr, value, .. } => {
                    check(&mut e, *addr, ii, Some(ptr));
                    check(&mut e, *value, ii, None);
                }
                Inst::Bin { op, a, b: y, .. } => {
                    let allowed: &[Class] = match op {
                        BinOp::And | BinOp::Or | BinOp::Xor => &[
                            Class::Bit,
                            Class::I8,
                            Class::I16,
                            Class::I32,
                            Class::I64,
                            Class::I128,
                        ],
                        BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv => floats,
                        _ => ints,
                    };
                    check(&mut e, *a, ii, Some(allowed));
                    check(&mut e, *y, ii, Some(allowed));
                    if class_of(&def, *a) != class_of(&def, *y) {
                        e.push(format!("v{} and v{} differ in class in b{bi}", a.0, y.0));
                    }
                }
                Inst::Ovf { a, b: y, .. } => {
                    check(&mut e, *a, ii, Some(ints));
                    check(&mut e, *y, ii, Some(ints));
                    if class_of(&def, *a) != class_of(&def, *y) {
                        e.push(format!("v{} and v{} differ in class in b{bi}", a.0, y.0));
                    }
                }
                Inst::Un { op, a, .. } => {
                    let allowed = match op {
                        super::ir::UnOp::FNeg
                        | super::ir::UnOp::FSqrt
                        | super::ir::UnOp::FAbs
                        | super::ir::UnOp::FFloor
                        | super::ir::UnOp::FCeil
                        | super::ir::UnOp::FTrunc => floats,
                        _ => ints,
                    };
                    check(&mut e, *a, ii, Some(allowed));
                }
                Inst::Conv { src, .. } => check(&mut e, *src, ii, Some(numbers)),
                Inst::Cmp { a, b: y, .. } => {
                    check(&mut e, *a, ii, None);
                    check(&mut e, *y, ii, None);
                    if class_of(&def, *a) != class_of(&def, *y) {
                        e.push(format!("v{} and v{} differ in class in b{bi}", a.0, y.0));
                    }
                }
                Inst::Select { cond, a, b: y, .. } => {
                    check(&mut e, *cond, ii, Some(bit));
                    check(&mut e, *a, ii, None);
                    check(&mut e, *y, ii, None);
                    if class_of(&def, *a) != class_of(&def, *y) {
                        e.push(format!("v{} and v{} differ in class in b{bi}", a.0, y.0));
                    }
                }
                Inst::Ext { src, .. } | Inst::Trunc { src, .. } => check(&mut e, *src, ii, None),
                Inst::PtrToInt { src, .. } => check(&mut e, *src, ii, Some(ptr)),
                Inst::IntToPtr { src, .. } => check(&mut e, *src, ii, Some(i64c)),
                Inst::PtrAdd { base, offset, .. } => {
                    check(&mut e, *base, ii, Some(ptr));
                    check(&mut e, *offset, ii, Some(i64c));
                }
                Inst::SetVar { var, src } => match f.vars.get(var.0 as usize) {
                    Some(c) => check(&mut e, *src, ii, Some(std::slice::from_ref(c))),
                    None => e.push(format!("no variable x{}", var.0)),
                },
                Inst::Call { callee, args, .. } => match m.sig(*callee) {
                    Some(s) => call_args(
                        &mut e,
                        s,
                        args,
                        ii,
                        &check,
                        &m.symbol(*callee).map_or(String::new(), |x| x.name.clone()),
                    ),
                    None => e.push(format!(
                        "a call to a symbol that is no function, {}",
                        callee.0
                    )),
                },
                Inst::CallIndirect {
                    addr, sig, args, ..
                } => {
                    check(&mut e, *addr, ii, Some(ptr));
                    call_args(&mut e, sig, args, ii, &check, "an indirect callee");
                }
                Inst::Copy { dst, src, .. } => {
                    check(&mut e, *dst, ii, Some(ptr));
                    check(&mut e, *src, ii, Some(ptr));
                }
                Inst::Zero { dst, .. } => check(&mut e, *dst, ii, Some(ptr)),
                Inst::Param { .. }
                | Inst::Const { .. }
                | Inst::Addr { .. }
                | Inst::SlotAddr { .. }
                | Inst::GetVar { .. }
                | Inst::Loc(_) => {}
            }
        }
        let end = b.insts.len();
        match &b.term {
            Term::Branch {
                cond,
                then,
                otherwise,
            } => {
                check(&mut e, *cond, end, Some(bit));
                for t in [then, otherwise] {
                    if t.index() >= n {
                        e.push(format!("a branch to no block b{}", t.0));
                    }
                }
            }
            Term::Jump(t) => {
                if t.index() >= n {
                    e.push(format!("a jump to no block b{}", t.0));
                }
            }
            Term::Switch {
                value,
                cases,
                default,
            } => {
                check(&mut e, *value, end, Some(ints));
                for (_, t) in cases.iter().map(|(k, t)| (k, t)).chain([(&0, default)]) {
                    if t.index() >= n {
                        e.push(format!("a switch to no block b{}", t.0));
                    }
                }
            }
            Term::Return(v) => match (v, own.ret) {
                (Some(v), Some(c)) => check(&mut e, *v, end, Some(std::slice::from_ref(&c))),
                (None, None) => {}
                (Some(_), None) => e.push(format!(
                    "b{bi} returns a value from a function that returns none"
                )),
                (None, Some(_)) => e.push(format!(
                    "b{bi} returns nothing from a function that returns a value"
                )),
            },
            Term::Fail => {
                if !own.fails() {
                    e.push(format!(
                        "b{bi} fails from a function that returns no failure bit"
                    ));
                }
            }
            Term::Unreachable => {}
        }
    }
    if e.is_empty() {
        Ok(())
    } else {
        Err(e)
    }
}

/// A call's `failed` value is its failure edge (4.2, `docs/architecture.md` §7.115): only the
/// condition of the branch that ends the call's block, the call its last instruction. So a backend
/// can make it an unwind edge — a landing pad continuing where the branch's `then` goes — or keep
/// it as a register on a target that returns the bit.
fn failure_edges(f: &Function, e: &mut Vec<String>) {
    let mut edges: BTreeMap<V, usize> = BTreeMap::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            let (Inst::Call {
                failed: Some(fv), ..
            }
            | Inst::CallIndirect {
                failed: Some(fv), ..
            }) = i
            else {
                continue;
            };
            edges.insert(*fv, bi);
            let ends = ii + 1 == b.insts.len()
                && matches!(b.term, Term::Branch { cond, .. } if cond == *fv);
            if !ends {
                e.push(format!(
                    "the failure edge v{} of a call in b{bi} is not the branch that ends its block",
                    fv.0
                ));
            }
        }
    }
    if edges.is_empty() {
        return;
    }
    for (bi, b) in f.blocks.iter().enumerate() {
        for v in b.insts.iter().flat_map(operands) {
            if edges.contains_key(&v) {
                e.push(format!(
                    "v{}, a call's failure edge, is used as a value in b{bi}",
                    v.0
                ));
            }
        }
        let used = match &b.term {
            Term::Branch { cond, .. } => edges.get(cond).is_some_and(|d| *d != bi).then_some(*cond),
            Term::Switch { value: v, .. } | Term::Return(Some(v)) => {
                edges.contains_key(v).then_some(*v)
            }
            Term::Jump(_) | Term::Return(None) | Term::Fail | Term::Unreachable => None,
        };
        if let Some(v) = used {
            e.push(format!(
                "v{}, a call's failure edge, is used as a value in b{bi}",
                v.0
            ));
        }
    }
}

/// The values an instruction reads.
fn operands(i: &Inst) -> Vec<V> {
    match i {
        Inst::Load { addr: a, .. }
        | Inst::Un { a, .. }
        | Inst::Conv { src: a, .. }
        | Inst::Ext { src: a, .. }
        | Inst::Trunc { src: a, .. }
        | Inst::PtrToInt { src: a, .. }
        | Inst::IntToPtr { src: a, .. }
        | Inst::SetVar { src: a, .. }
        | Inst::Zero { dst: a, .. } => vec![*a],
        Inst::Store {
            addr: a, value: b, ..
        }
        | Inst::Bin { a, b, .. }
        | Inst::Ovf { a, b, .. }
        | Inst::Cmp { a, b, .. }
        | Inst::PtrAdd {
            base: a, offset: b, ..
        }
        | Inst::Copy { dst: a, src: b, .. } => vec![*a, *b],
        Inst::Select { cond, a, b, .. } => vec![*cond, *a, *b],
        Inst::Call { args, .. } => args.clone(),
        Inst::CallIndirect { addr, args, .. } => {
            std::iter::once(*addr).chain(args.iter().copied()).collect()
        }
        Inst::Param { .. }
        | Inst::Const { .. }
        | Inst::Addr { .. }
        | Inst::SlotAddr { .. }
        | Inst::GetVar { .. }
        | Inst::Loc(_) => Vec::new(),
    }
}

/// The check of one use: the value, where in its block it is used, and the classes it may be.
type Check<'a> = dyn Fn(&mut Vec<String>, V, usize, Option<&[Class]>) + 'a;

fn call_args(e: &mut Vec<String>, s: &Sig, args: &[V], at: usize, check: &Check<'_>, name: &str) {
    if s.params.len() != args.len() {
        e.push(format!(
            "`{name}` is given {} arguments and takes {}",
            args.len(),
            s.params.len()
        ));
        return;
    }
    for (a, c) in args.iter().zip(&s.params) {
        check(e, *a, at, Some(std::slice::from_ref(c)));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::expect_used)]

    use super::module;
    use crate::op::ir::{
        Block, Class, Cmp, Conv, Function, Inst, Linkage, Module, Sig, SymId, SymKind, Symbol,
        Term, Var, B, V,
    };

    /// `f(a: i64) -> i64`, and an imported `g(i64, ptr)`, with `body` as `f`'s blocks.
    fn with(blocks: Vec<Block>, vars: Vec<Class>) -> Module {
        let func = |params: Vec<Class>, ret: Option<Class>, linkage| Symbol {
            name: String::new(),
            linkage,
            kind: SymKind::Func {
                sig: Sig {
                    params,
                    ret,
                    conv: Conv::Nazm,
                },
            },
        };
        Module {
            source: "t.nz".to_owned(),
            pointer: 8,
            symbols: vec![
                func(vec![Class::I64], Some(Class::I64), Linkage::Local),
                func(vec![Class::I64, Class::Ptr], None, Linkage::Import),
            ],
            functions: vec![Function {
                sym: SymId(0),
                name: "f".to_owned(),
                vars,
                slots: Vec::new(),
                var_names: Vec::new(),
                slot_names: Vec::new(),
                blocks,
                values: 16,
                at: None,
                debug_vars: Vec::new(),
                section: None,
            }],
            data: Vec::new(),
            needs: crate::Needs::default(),
        }
    }

    fn block(insts: Vec<Inst>, term: Term) -> Block {
        Block { insts, term }
    }

    fn param() -> Inst {
        Inst::Param {
            dst: V(0),
            index: 0,
        }
    }

    fn refused(m: &Module, says: &str) {
        let e = module(m).expect_err("a broken module validated");
        assert!(
            e.iter().any(|x| x.contains(says)),
            "{e:?} does not say {says}"
        );
    }

    #[test]
    fn a_well_formed_module_validates() {
        // A value used across a branch through a variable, and one used in a block it dominates.
        let m = with(
            vec![
                block(
                    vec![
                        param(),
                        Inst::SetVar {
                            var: Var(0),
                            src: V(0),
                        },
                        Inst::Const {
                            dst: V(1),
                            class: Class::I64,
                            value: 0,
                        },
                        Inst::Cmp {
                            dst: V(2),
                            op: Cmp::Lt,
                            a: V(0),
                            b: V(1),
                        },
                    ],
                    Term::Branch {
                        cond: V(2),
                        then: B(1),
                        otherwise: B(2),
                    },
                ),
                block(
                    vec![Inst::GetVar {
                        dst: V(3),
                        var: Var(0),
                    }],
                    Term::Return(Some(V(3))),
                ),
                block(Vec::new(), Term::Return(Some(V(1)))),
            ],
            vec![Class::I64],
        );
        assert_eq!(module(&m), Ok(()));
    }

    #[test]
    fn a_value_used_where_its_definition_does_not_reach_is_refused() {
        // `v1` is defined in b1 and used in b2, which b1 does not dominate.
        let m = with(
            vec![
                block(
                    vec![
                        param(),
                        Inst::Cmp {
                            dst: V(2),
                            op: Cmp::Eq,
                            a: V(0),
                            b: V(0),
                        },
                    ],
                    Term::Branch {
                        cond: V(2),
                        then: B(1),
                        otherwise: B(2),
                    },
                ),
                block(
                    vec![Inst::Const {
                        dst: V(1),
                        class: Class::I64,
                        value: 1,
                    }],
                    Term::Jump(B(2)),
                ),
                block(Vec::new(), Term::Return(Some(V(1)))),
            ],
            Vec::new(),
        );
        refused(&m, "does not reach");
    }

    #[test]
    fn a_value_used_before_it_is_defined_in_its_block_is_refused() {
        let m = with(
            vec![block(
                vec![
                    param(),
                    Inst::Bin {
                        dst: V(2),
                        op: crate::op::ir::BinOp::Add,
                        a: V(0),
                        b: V(1),
                    },
                    Inst::Const {
                        dst: V(1),
                        class: Class::I64,
                        value: 1,
                    },
                ],
                Term::Return(Some(V(2))),
            )],
            Vec::new(),
        );
        refused(&m, "does not reach");
    }

    #[test]
    fn a_class_mismatch_is_refused() {
        // A branch on an `i64`, and a pointer stored into an `i64` variable.
        let m = with(
            vec![
                block(
                    vec![param()],
                    Term::Branch {
                        cond: V(0),
                        then: B(1),
                        otherwise: B(1),
                    },
                ),
                block(Vec::new(), Term::Return(Some(V(0)))),
            ],
            Vec::new(),
        );
        refused(&m, "where bit is needed");
        let m = with(
            vec![block(
                vec![
                    param(),
                    Inst::IntToPtr {
                        dst: V(1),
                        src: V(0),
                    },
                    Inst::SetVar {
                        var: Var(0),
                        src: V(1),
                    },
                ],
                Term::Return(Some(V(0))),
            )],
            vec![Class::I64],
        );
        refused(&m, "where i64 is needed");
    }

    #[test]
    fn a_call_that_does_not_match_its_callee_is_refused() {
        // Too few arguments, then the right count with a wrong class, then a result of nothing.
        let call = |args: Vec<V>, dst: Option<V>| {
            with(
                vec![block(
                    vec![
                        param(),
                        Inst::Call {
                            dst,
                            failed: None,
                            callee: SymId(1),
                            args,
                        },
                    ],
                    Term::Return(Some(V(0))),
                )],
                Vec::new(),
            )
        };
        refused(&call(vec![V(0)], None), "is given 1 arguments and takes 2");
        refused(&call(vec![V(0), V(0)], None), "where ptr is needed");
        refused(&call(Vec::new(), Some(V(5))), "which returns nothing");
    }

    #[test]
    fn a_failure_bit_only_a_nazm_function_returns_is_refused_elsewhere() {
        // A Nazm function fails; a plain one does not, and a call to one has no bit to take.
        let failing = with(vec![block(vec![param()], Term::Fail)], Vec::new());
        assert!(module(&failing).is_ok(), "{:?}", module(&failing));
        let plain = |mut m: Module, which: usize| {
            if let SymKind::Func { sig } = &mut m.symbols[which].kind {
                sig.conv = Conv::Plain;
            }
            m
        };
        refused(
            &plain(failing, 0),
            "fails from a function that returns no failure bit",
        );
        let call = with(edge(Vec::new(), V(2)), Vec::new());
        assert!(module(&call).is_ok(), "{:?}", module(&call));
        refused(&plain(call, 1), "the failure bit of");
    }

    /// `f` calling `g` with the bit, `tail` after the call, its block ending in a branch on `cond`
    /// to a failure exit or a return of the parameter.
    fn edge(tail: Vec<Inst>, cond: V) -> Vec<Block> {
        let mut insts = vec![
            param(),
            Inst::Const {
                dst: V(1),
                class: Class::Ptr,
                value: 0,
            },
            Inst::Call {
                dst: None,
                failed: Some(V(2)),
                callee: SymId(1),
                args: vec![V(0), V(1)],
            },
        ];
        insts.extend(tail);
        vec![
            block(
                insts,
                Term::Branch {
                    cond,
                    then: B(1),
                    otherwise: B(2),
                },
            ),
            block(Vec::new(), Term::Fail),
            block(Vec::new(), Term::Return(Some(V(0)))),
        ]
    }

    #[test]
    fn a_failure_edge_is_only_the_branch_that_ends_its_call_s_block() {
        // 4.2: the `failed` value is the call's failure edge, which a backend may make an unwind
        // edge — so it is never a value anything else reads, nor followed by an instruction.
        let after = Inst::Const {
            dst: V(3),
            class: Class::Bit,
            value: 0,
        };
        refused(
            &with(edge(vec![after.clone()], V(3)), Vec::new()),
            "is not the branch that ends its block",
        );
        refused(
            &with(edge(vec![after], V(2)), Vec::new()),
            "is not the branch that ends its block",
        );
        let read = Inst::Select {
            dst: V(4),
            cond: V(2),
            a: V(0),
            b: V(0),
        };
        let mut blocks = edge(Vec::new(), V(2));
        blocks[2].insts.push(read);
        refused(
            &with(blocks, Vec::new()),
            "a call's failure edge, is used as a value",
        );
        let mut blocks = edge(Vec::new(), V(2));
        blocks[2].term = Term::Branch {
            cond: V(2),
            then: B(1),
            otherwise: B(1),
        };
        refused(
            &with(blocks, Vec::new()),
            "a call's failure edge, is used as a value",
        );
    }

    #[test]
    fn a_return_that_does_not_match_the_function_is_refused() {
        let m = with(vec![block(vec![param()], Term::Return(None))], Vec::new());
        refused(&m, "returns nothing from a function that returns a value");
    }

    #[test]
    fn a_jump_to_no_block_a_second_definition_and_a_late_parameter_are_refused() {
        let m = with(vec![block(vec![param()], Term::Jump(B(7)))], Vec::new());
        refused(&m, "a jump to no block b7");
        let m = with(
            vec![block(
                vec![
                    param(),
                    Inst::Const {
                        dst: V(0),
                        class: Class::I64,
                        value: 1,
                    },
                ],
                Term::Return(Some(V(0))),
            )],
            Vec::new(),
        );
        refused(&m, "v0 is defined twice");
        let m = with(
            vec![
                block(Vec::new(), Term::Jump(B(1))),
                block(vec![param()], Term::Return(Some(V(0)))),
            ],
            Vec::new(),
        );
        refused(&m, "a parameter outside the entry block");
    }
}
