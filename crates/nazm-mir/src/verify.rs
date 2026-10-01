//! MIR's invariants, checked.
//!
//! `docs/architecture.md` §7.42 lists them; this is the list, run. Two passes per function: a
//! structural and typing pass over every statement and terminator, then a forward dataflow over
//! the locals that own something, which is what makes the cleanup law checkable rather than
//! merely stated — no read of an empty local, no overwrite of an owned one, and nothing still
//! owned at a `return` or an `unwind`.
//!
//! It reads MIR and nothing else, never changes it, and gives the same answer every time.

use nazm_sema::{Intrinsic, Shape, Type};

use crate::ir::{
    BlockId, FieldRef, Function, LocalId, LocalOrigin, LocalTy, Operand, Program, Rvalue,
    StatementKind, Terminator, Types,
};

/// Every broken invariant, as a sentence naming the function and the place. Empty when MIR is
/// well formed.
#[must_use]
pub fn verify(program: &Program) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, f) in program.functions.iter().enumerate() {
        if f.id.index() != i {
            errors.push(format!(
                "`{}`: its id is {}, at position {i}",
                f.name, f.id.0
            ));
        }
        let mut out = Vec::new();
        if f.foreign.is_some() {
            // A foreign function has its parameters and no body (N42).
            if !f.blocks.is_empty() || f.locals.len() != f.params as usize {
                out.push("a foreign function with a body".to_owned());
            }
            errors.extend(out.into_iter().map(|e| format!("`{}`: {e}", f.name)));
            continue;
        }
        structure(program, f, &mut out);
        if out.is_empty() {
            ownership(&program.types, f, &mut out);
        }
        errors.extend(out.into_iter().map(|e| format!("`{}`: {e}", f.name)));
    }
    if let Some(e) = program.entry {
        if program.function(e).is_none() {
            errors.push("the entry names no function".to_owned());
        }
    }
    errors
}

struct Ctx<'a> {
    program: &'a Program,
    f: &'a Function,
}

impl Ctx<'_> {
    fn local(&self, l: LocalId) -> Option<LocalTy> {
        self.f.local(l).map(|d| d.ty)
    }

    fn value(&self, l: LocalId) -> Option<Type> {
        self.local(l).and_then(LocalTy::value)
    }

    fn operand(&self, o: &Operand) -> Option<Type> {
        match o {
            Operand::Const(c) => Some(c.ty()),
            Operand::Local(l) => self.value(*l),
        }
    }

    fn managed(&self, t: Type) -> bool {
        self.program.types.managed(t)
    }

    fn block_exists(&self, b: BlockId) -> bool {
        b.index() < self.f.blocks.len()
    }

    /// The type a primitive's shape stands for, given its element type.
    fn shape(&self, s: Shape, elem: Option<Type>) -> Option<Type> {
        match s {
            Shape::T => elem,
            Shape::VecT => elem.and_then(|e| self.program.types.vec_of(e)),
            other => other.monomorphic(),
        }
    }
}

fn structure(program: &Program, f: &Function, out: &mut Vec<String>) {
    let cx = Ctx { program, f };
    if f.blocks.is_empty() {
        out.push("it has no blocks".to_owned());
        return;
    }
    if (f.params as usize) > f.locals.len() {
        out.push("it declares more parameters than locals".to_owned());
    }
    for (i, l) in f.locals.iter().enumerate() {
        let is_param = i < f.params as usize;
        if is_param != (l.origin == LocalOrigin::Param) {
            out.push(format!("local _{i} is misplaced as a parameter"));
        }
        if is_param && l.ty == LocalTy::Tasks {
            out.push(format!("parameter _{i} is a task list"));
        }
    }
    // Which (scrutinee, variant) each switch arm block is the arm of.
    let mut arm_of: Vec<Option<(LocalId, u32)>> = vec![None; f.blocks.len()];
    for b in &f.blocks {
        if let Terminator::Switch {
            scrutinee, arms, ..
        } = &b.term
        {
            for (v, target) in arms {
                if let Some(slot) = arm_of.get_mut(target.index()) {
                    *slot = Some((*scrutinee, *v));
                }
            }
        }
    }
    for (bi, b) in f.blocks.iter().enumerate() {
        let here = |what: &str| format!("bb{bi}: {what}");
        for (si, s) in b.stmts.iter().enumerate() {
            let at = |what: String| format!("bb{bi}[{si}]: {what}");
            if let Some(u) = s.kind.unwind() {
                if !cx.block_exists(u) {
                    out.push(at(format!(
                        "a failure edge to bb{} that does not exist",
                        u.0
                    )));
                }
            }
            match &s.kind {
                StatementKind::Assign { dst, value, unwind } => {
                    if value.can_fail() != unwind.is_some() {
                        out.push(at(if value.can_fail() {
                            "an operation that can fail has no failure edge".to_owned()
                        } else {
                            "an operation that cannot fail has a failure edge".to_owned()
                        }));
                    }
                    let Some(want) = cx.value(*dst) else {
                        out.push(at(format!("assigns _{} which is not a value local", dst.0)));
                        continue;
                    };
                    if let Some(e) = rvalue(&cx, value, want, arm_of.get(bi).copied().flatten()) {
                        out.push(at(e));
                    }
                }
                StatementKind::SetField { base, path, value } => {
                    let Some(mut t) = cx.value(*base) else {
                        out.push(at("a field write on a local that does not exist".to_owned()));
                        continue;
                    };
                    let mut ok = true;
                    for FieldRef { record, field } in path {
                        if t != Type::Record(*record) {
                            out.push(at("a field path that does not follow its types".to_owned()));
                            ok = false;
                            break;
                        }
                        match program.types.field(*record, *field) {
                            Some(ft) => t = ft,
                            None => {
                                out.push(at("a field its record does not have".to_owned()));
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && cx.value(*value) != Some(t) {
                        out.push(at("a field written with a value of another type".to_owned()));
                    }
                    if path.is_empty() {
                        out.push(at("a field write with no field".to_owned()));
                    }
                }
                StatementKind::Drop(l) => match cx.f.local(*l) {
                    None => out.push(at(format!("drops _{} which does not exist", l.0))),
                    Some(d) if d.origin == LocalOrigin::Param => {
                        out.push(at(format!(
                            "drops parameter _{}, which it only borrows",
                            l.0
                        )));
                    }
                    Some(d) if d.ty == LocalTy::Tasks => {
                        out.push(at(format!(
                            "drops task list _{} instead of joining it",
                            l.0
                        )));
                    }
                    Some(_) => {}
                },
                StatementKind::ScopeOpen { list, .. } | StatementKind::ScopeJoin { list } => {
                    if cx.local(*list) != Some(LocalTy::Tasks) {
                        out.push(at(format!("_{} is not a task list", list.0)));
                    }
                }
                StatementKind::Spawn {
                    list, callee, args, ..
                } => {
                    if cx.local(*list) != Some(LocalTy::Tasks) {
                        out.push(at(format!("_{} is not a task list", list.0)));
                    }
                    match program.function(*callee) {
                        None => out.push(at("spawns a function that does not exist".to_owned())),
                        Some(g) => {
                            if let Some(e) = arguments(&cx, g, args) {
                                out.push(at(e));
                            }
                        }
                    }
                }
                StatementKind::CheckFailed { .. } | StatementKind::StackCheck { .. } => {}
            }
        }
        for t in b.term.successors() {
            if !cx.block_exists(t) {
                out.push(here(&format!("jumps to bb{} which does not exist", t.0)));
            }
        }
        match &b.term {
            Terminator::Branch { cond, .. } => {
                if cx.operand(cond) != Some(Type::Bool) {
                    out.push(here("a branch on something that is not a `Bool`"));
                }
            }
            Terminator::Switch {
                scrutinee,
                enumeration,
                arms,
            } => {
                if cx.value(*scrutinee) != Some(Type::Enum(*enumeration)) {
                    out.push(here("a switch whose scrutinee is not of its enum"));
                }
                match program.types.enumeration(*enumeration) {
                    None => out.push(here("a switch on an enum the program does not have")),
                    Some(e) => {
                        let mut seen = vec![false; e.variants.len()];
                        let mut bad = arms.len() != e.variants.len();
                        for (v, _) in arms {
                            match seen.get_mut(*v as usize) {
                                Some(s) if !*s => *s = true,
                                _ => bad = true,
                            }
                        }
                        if bad {
                            out.push(here(
                                "a switch that does not name each variant exactly once",
                            ));
                        }
                    }
                }
            }
            Terminator::Return(l) => {
                if cx.value(*l) != Some(f.ret) {
                    out.push(here("returns a local that is not of the function's type"));
                }
            }
            Terminator::Exit(code) => {
                if cx.operand(code) != Some(Type::Int) {
                    out.push(here("exits with a status that is not an `Int`"));
                }
            }
            Terminator::Goto(_) | Terminator::Unwind => {}
        }
    }
}

/// Whether a call's arguments fit its callee's parameters.
fn arguments(cx: &Ctx<'_>, callee: &Function, args: &[Operand]) -> Option<String> {
    let params: Vec<Type> = callee.param_types().collect();
    if params.len() != args.len() {
        return Some(format!(
            "calls `{}` with {} arguments where it takes {}",
            callee.name,
            args.len(),
            params.len()
        ));
    }
    for (i, (a, p)) in args.iter().zip(&params).enumerate() {
        if cx.operand(a) != Some(*p) {
            return Some(format!(
                "passes `{}` an argument {i} of the wrong type",
                callee.name
            ));
        }
    }
    None
}

/// What is wrong with `value` written into a local of type `want`, if anything.
fn rvalue(cx: &Ctx<'_>, value: &Rvalue, want: Type, arm: Option<(LocalId, u32)>) -> Option<String> {
    let types = &cx.program.types;
    let is = |t: Option<Type>| t == Some(want);
    match value {
        Rvalue::Use(o) => {
            if !is(cx.operand(o)) {
                return Some("a use of a value of another type".to_owned());
            }
            if let Operand::Local(l) = o {
                if cx.managed(want) {
                    return Some(format!(
                        "reads managed _{} with neither `copy` nor `move`",
                        l.0
                    ));
                }
            }
            None
        }
        Rvalue::Copy(l) | Rvalue::Move(l) => {
            (!is(cx.value(*l))).then(|| "a copy or move of a value of another type".to_owned())
        }
        Rvalue::Neg(a) => (cx.operand(a) != Some(Type::Int) || want != Type::Int)
            .then(|| "a negation that is not of `Int`".to_owned()),
        Rvalue::Arith(_, a, b) => (cx.operand(a) != Some(Type::Int)
            || cx.operand(b) != Some(Type::Int)
            || want != Type::Int)
            .then(|| "arithmetic that is not on `Int`".to_owned()),
        Rvalue::Compare {
            op,
            law,
            operand,
            lhs,
            rhs,
        } => {
            if want != Type::Bool {
                return Some("a comparison written into something that is not a `Bool`".to_owned());
            }
            if cx.operand(lhs) != Some(*operand) || cx.operand(rhs) != Some(*operand) {
                return Some("a comparison of operands not of its operand type".to_owned());
            }
            (nazm_cir::Law::of(*op, *operand) != Some(*law))
                .then(|| "a comparison under the wrong law for its type".to_owned())
        }
        Rvalue::Call { callee, args } => match cx.program.function(*callee) {
            None => Some("a call to a function that does not exist".to_owned()),
            Some(g) => {
                if let Some(e) = arguments(cx, g, args) {
                    return Some(e);
                }
                (g.ret != want).then(|| format!("a call to `{}` of the wrong result type", g.name))
            }
        },
        Rvalue::Primitive { op, elem, args } => primitive(cx, *op, *elem, args, want),
        Rvalue::Record { record, fields } => {
            let Some(r) = types.record(*record) else {
                return Some("a record the program does not have".to_owned());
            };
            if want != Type::Record(*record) {
                return Some("a record written into a local of another type".to_owned());
            }
            complete(cx, &r.fields, fields)
        }
        Rvalue::Variant {
            enumeration,
            variant,
            fields,
        } => {
            let Some(v) = types
                .enumeration(*enumeration)
                .and_then(|e| e.variants.get(*variant as usize))
            else {
                return Some("a variant its enum does not have".to_owned());
            };
            if want != Type::Enum(*enumeration) {
                return Some("a variant written into a local of another type".to_owned());
            }
            complete(cx, &v.fields, fields)
        }
        Rvalue::Field { base, field } => {
            if cx.operand(base) != Some(Type::Record(field.record)) {
                return Some("a field read from something that is not its record".to_owned());
            }
            (!is(types.field(field.record, field.field)))
                .then(|| "a field read into a local of another type".to_owned())
        }
        Rvalue::Payload {
            base,
            enumeration,
            variant,
            field,
        } => {
            if cx.value(*base) != Some(Type::Enum(*enumeration)) {
                return Some("a payload read from something that is not its enum".to_owned());
            }
            if arm != Some((*base, *variant)) {
                return Some(format!(
                    "a payload of variant {variant} read outside the arm that matched it"
                ));
            }
            (!is(types.payload(*enumeration, *variant, *field)))
                .then(|| "a payload read into a local of another type".to_owned())
        }
    }
}

/// Whether an aggregate's initialisers name each field exactly once, with the right types.
fn complete(
    cx: &Ctx<'_>,
    declared: &[crate::ir::FieldInfo],
    fields: &[(u32, LocalId)],
) -> Option<String> {
    if fields.len() != declared.len() {
        return Some(format!(
            "an aggregate built from {} values where it has {} fields",
            fields.len(),
            declared.len()
        ));
    }
    let mut seen = vec![false; declared.len()];
    for (field, l) in fields {
        match seen.get_mut(*field as usize) {
            Some(s) if !*s => *s = true,
            _ => return Some(format!("an aggregate that names field {field} twice")),
        }
        let want = declared.get(*field as usize).map(|f| f.ty);
        if cx.value(*l) != want {
            return Some(format!("an aggregate field {field} of the wrong type"));
        }
    }
    None
}

fn primitive(
    cx: &Ctx<'_>,
    op: Intrinsic,
    elem: Option<Type>,
    args: &[Operand],
    want: Type,
) -> Option<String> {
    if op == Intrinsic::ExitWith {
        return Some("`exit_with` as a value; it is a terminator".to_owned());
    }
    let generic = op
        .params()
        .iter()
        .chain(std::iter::once(&op.ret()))
        .any(|s| matches!(s, Shape::T | Shape::VecT));
    if generic != elem.is_some() {
        return Some(format!("`{}` with the wrong element type", op.name()));
    }
    let params = op.params();
    if params.len() != args.len() {
        return Some(format!(
            "`{}` with {} arguments where it takes {}",
            op.name(),
            args.len(),
            params.len()
        ));
    }
    for (i, (a, s)) in args.iter().zip(params).enumerate() {
        let Some(p) = cx.shape(*s, elem) else {
            return Some(format!(
                "`{}` over a `Vec` the program does not have",
                op.name()
            ));
        };
        if cx.operand(a) != Some(p) {
            return Some(format!(
                "`{}` given argument {i} of the wrong type",
                op.name()
            ));
        }
    }
    (cx.shape(op.ret(), elem) != Some(want))
        .then(|| format!("`{}` written into a local of another type", op.name()))
}

// --- ownership ----------------------------------------------------------------------------------

/// The state of every tracked local at one point, as two bit sets: which locals *may* own a
/// value, and which *may* be empty. Owned is `may_own` alone, empty is `may_be_empty` alone,
/// and both at once is *either* — two paths disagreed. A join is the union of each set, so the
/// dataflow costs a word per 64 tracked locals per block rather than a byte per local.
#[derive(Debug, Clone, PartialEq, Eq)]
struct St {
    may_own: Vec<u64>,
    may_be_empty: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum One {
    Empty,
    Owned,
    Either,
}

impl St {
    fn empty(n: usize) -> Self {
        let words = n.div_ceil(64);
        let mut may_be_empty = vec![u64::MAX; words];
        if let Some(last) = may_be_empty.last_mut() {
            let tail = n % 64;
            if tail != 0 {
                *last = (1u64 << tail) - 1;
            }
        }
        Self {
            may_own: vec![0; words],
            may_be_empty,
        }
    }

    fn get(&self, i: usize) -> One {
        let bit = |v: &[u64]| {
            v.get(i.div_euclid(64))
                .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
        };
        match (bit(&self.may_own), bit(&self.may_be_empty)) {
            (true, false) => One::Owned,
            (false, true) | (false, false) => One::Empty,
            (true, true) => One::Either,
        }
    }

    fn set(&mut self, i: usize, v: One) {
        let (own, empty) = match v {
            One::Owned => (true, false),
            One::Empty => (false, true),
            One::Either => (true, true),
        };
        let mask = 1u64 << (i % 64);
        if let Some(w) = self.may_own.get_mut(i.div_euclid(64)) {
            if own {
                *w |= mask;
            } else {
                *w &= !mask;
            }
        }
        if let Some(w) = self.may_be_empty.get_mut(i.div_euclid(64)) {
            if empty {
                *w |= mask;
            } else {
                *w &= !mask;
            }
        }
    }

    /// Join `other` into this, reporting whether anything changed.
    fn join(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (a, b) in self.may_own.iter_mut().zip(&other.may_own) {
            let n = *a | *b;
            changed |= n != *a;
            *a = n;
        }
        for (a, b) in self.may_be_empty.iter_mut().zip(&other.may_be_empty) {
            let n = *a | *b;
            changed |= n != *a;
            *a = n;
        }
        changed
    }

    /// Every tracked index that may still own something.
    fn owning(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (w, word) in self.may_own.iter().enumerate() {
            let mut bits = *word;
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                out.push(w * 64 + b);
                bits &= bits - 1;
            }
        }
        out
    }
}

/// The forward dataflow: which tracked locals are empty, owned, or either, at each point.
fn ownership(types: &Types, f: &Function, out: &mut Vec<String>) {
    // Tracked: every non-parameter local that owns something, and every task list, each by a
    // dense index.
    let mut index: Vec<Option<usize>> = vec![None; f.locals.len()];
    let mut of_index: Vec<usize> = Vec::new();
    for (i, l) in f.locals.iter().enumerate() {
        if i >= f.params as usize && types.managed_local(l.ty) {
            if let Some(slot) = index.get_mut(i) {
                *slot = Some(of_index.len());
            }
            of_index.push(i);
        }
    }
    let tracked = of_index.len();
    let param = |l: LocalId| l.index() < f.params as usize;
    let n = f.blocks.len();
    let mut entry: Vec<Option<St>> = vec![None; n];
    if let Some(first) = entry.get_mut(0) {
        *first = Some(St::empty(tracked));
    }
    // Visit order: source blocks by number, then the failure blocks. Every fallible statement
    // flows into the shared unwind block, and it flows nowhere; visited last, it is visited
    // once rather than once per new fact, which would make the pass quadratic in a large
    // function.
    let rank = |b: usize| -> (u8, usize) {
        let late = f
            .blocks
            .get(b)
            .is_some_and(|x| x.origin != crate::ir::BlockOrigin::Source);
        (u8::from(late), b)
    };
    let mut work: std::collections::BTreeSet<(u8, usize)> = std::collections::BTreeSet::new();
    work.insert(rank(0));
    let mut reported: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let flow = |entry: &mut Vec<Option<St>>,
                work: &mut std::collections::BTreeSet<(u8, usize)>,
                to: BlockId,
                st: &St| {
        let Some(slot) = entry.get_mut(to.index()) else {
            return;
        };
        match slot {
            None => {
                *slot = Some(st.clone());
                work.insert(rank(to.index()));
            }
            Some(old) => {
                if old.join(st) {
                    work.insert(rank(to.index()));
                }
            }
        }
    };
    let mut steps = 0usize;
    while let Some((_, bi)) = work.pop_first() {
        steps += 1;
        if steps > (n + 1) * (2 * tracked + 2) {
            reported.insert("the ownership dataflow did not settle".to_owned());
            break;
        }
        let Some(Some(start)) = entry.get(bi).cloned() else {
            continue;
        };
        let Some(b) = f.blocks.get(bi) else {
            continue;
        };
        let mut st = start;
        let mut report = |e: String| {
            reported.insert(e);
        };
        let read = |st: &St, l: LocalId, what: &str, report: &mut dyn FnMut(String)| {
            if let Some(Some(i)) = index.get(l.index()) {
                match st.get(*i) {
                    One::Owned => {}
                    One::Empty => report(format!("bb{bi}: {what} _{} when it is empty", l.0)),
                    One::Either => {
                        report(format!("bb{bi}: {what} _{} when it may be empty", l.0));
                    }
                }
            }
        };
        let set = |st: &mut St, l: LocalId, v: One| {
            if let Some(Some(i)) = index.get(l.index()) {
                st.set(*i, v);
            }
        };
        for s in &b.stmts {
            if let Some(u) = s.kind.unwind() {
                flow(&mut entry, &mut work, u, &st);
            }
            match &s.kind {
                StatementKind::Assign { dst, value, .. } => {
                    for o in rvalue_reads(value) {
                        read(&st, o, "reads", &mut report);
                    }
                    match value {
                        Rvalue::Move(l) => {
                            if param(*l) {
                                report(format!("bb{bi}: moves parameter _{}", l.0));
                            }
                            read(&st, *l, "moves", &mut report);
                            set(&mut st, *l, One::Empty);
                        }
                        Rvalue::Copy(l) => read(&st, *l, "copies", &mut report),
                        Rvalue::Record { fields, .. } | Rvalue::Variant { fields, .. } => {
                            for (_, l) in fields {
                                read(&st, *l, "moves", &mut report);
                                set(&mut st, *l, One::Empty);
                            }
                        }
                        _ => {}
                    }
                    if let Some(Some(i)) = index.get(dst.index()) {
                        if st.get(*i) != One::Empty {
                            report(format!(
                                "bb{bi}: overwrites _{} while it may still own a value",
                                dst.0
                            ));
                        }
                        st.set(*i, One::Owned);
                    }
                }
                StatementKind::SetField { base, value, .. } => {
                    read(&st, *base, "writes a field of", &mut report);
                    read(&st, *value, "moves", &mut report);
                    set(&mut st, *value, One::Empty);
                }
                StatementKind::Drop(l) => set(&mut st, *l, One::Empty),
                StatementKind::ScopeOpen { list, .. } => {
                    if let Some(Some(i)) = index.get(list.index()) {
                        if st.get(*i) != One::Empty {
                            report(format!("bb{bi}: opens task list _{} twice", list.0));
                        }
                    }
                    set(&mut st, *list, One::Owned);
                }
                StatementKind::Spawn { list, args, .. } => {
                    read(&st, *list, "spawns into", &mut report);
                    for a in args {
                        if let Operand::Local(l) = a {
                            read(&st, *l, "passes", &mut report);
                        }
                    }
                }
                StatementKind::ScopeJoin { list } => {
                    read(&st, *list, "joins", &mut report);
                    set(&mut st, *list, One::Empty);
                }
                StatementKind::CheckFailed { .. } | StatementKind::StackCheck { .. } => {}
            }
        }
        match &b.term {
            Terminator::Branch { cond, .. } => {
                if let Operand::Local(l) = cond {
                    read(&st, *l, "branches on", &mut report);
                }
            }
            Terminator::Switch { scrutinee, .. } => {
                read(&st, *scrutinee, "switches on", &mut report);
            }
            Terminator::Return(l) => {
                read(&st, *l, "returns", &mut report);
                let returned = index.get(l.index()).copied().flatten();
                for i in st.owning() {
                    if Some(i) != returned {
                        let local = of_index.get(i).copied().unwrap_or(0);
                        report(format!(
                            "bb{bi}: returns while _{local} may still own a value (a leak)"
                        ));
                    }
                }
            }
            Terminator::Unwind => {
                for i in st.owning() {
                    let local = of_index.get(i).copied().unwrap_or(0);
                    report(format!(
                        "bb{bi}: unwinds while _{local} may still own a value (a leak)"
                    ));
                }
            }
            Terminator::Exit(_) | Terminator::Goto(_) => {}
        }
        for t in b.term.successors() {
            flow(&mut entry, &mut work, t, &st);
        }
    }
    out.extend(reported);
}

/// The locals an rvalue reads through operands (borrowed), not those it moves or copies.
fn rvalue_reads(v: &Rvalue) -> Vec<LocalId> {
    let ops: Vec<&Operand> = match v {
        Rvalue::Use(o) | Rvalue::Neg(o) => vec![o],
        Rvalue::Arith(_, a, b) => vec![a, b],
        Rvalue::Compare { lhs, rhs, .. } => vec![lhs, rhs],
        Rvalue::Call { args, .. } | Rvalue::Primitive { args, .. } => args.iter().collect(),
        Rvalue::Field { base, .. } => vec![base],
        Rvalue::Payload { base, .. } => return vec![*base],
        Rvalue::Copy(_) | Rvalue::Move(_) | Rvalue::Record { .. } | Rvalue::Variant { .. } => {
            Vec::new()
        }
    };
    ops.into_iter()
        .filter_map(|o| match o {
            Operand::Local(l) => Some(*l),
            Operand::Const(_) => None,
        })
        .collect()
}
